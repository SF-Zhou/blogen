#!/usr/bin/env python3
"""Release bookkeeping for blogen.

The release workflow uses this script instead of hand-editing versions so that
the crate version, the changelog and the git tag can never drift apart.

Subcommands
-----------

``current``
    Print the version declared in ``Cargo.toml``.

``bump --part {patch,minor,major}``
    Compute the next version, write it to ``Cargo.toml`` and move the
    ``[Unreleased]`` changelog entries into a new version section.

``set --version X.Y.Z``
    Same as ``bump`` but with an explicit (possibly pre-release) version.

``check --tag vX.Y.Z``
    Validate that a tag, the crate version and the changelog agree.

``notes --version X.Y.Z [--out FILE]``
    Print the changelog section of a release, for use as GitHub release notes.

Only the Python standard library is used so the script runs on every GitHub
Actions runner without any setup.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ``X.Y.Z`` with an optional pre-release and build-metadata suffix. Build
# metadata (``+...``) is rejected by the release tooling below because ``+``
# is awkward in download URLs.
_SEMVER_RE = re.compile(
    r"^(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)"
    r"(?P<pre>-[0-9A-Za-z.-]+)?(?P<build>\+[0-9A-Za-z.-]+)?$"
)
_CHANGELOG_HEADING_RE = re.compile(
    r"(?m)^## \[([^\]\n]+)\](?: - ([^\]\n]+))?[ \t]*$"
)
_PACKAGE_HEADING_RE = re.compile(r"(?m)^\[package\][ \t]*$")
_NEXT_SECTION_RE = re.compile(r"(?m)^\[")
_VERSION_LINE_RE = re.compile(r'(?m)^version[ \t]*=[ \t]*"([^"]+)"[ \t]*$')


class ReleaseError(Exception):
    """A user-facing release error."""


def repo_root() -> Path:
    """Return the repository root (the directory containing ``Cargo.toml``)."""
    here = Path(__file__).resolve()
    for candidate in (here.parent, *here.parents):
        if (candidate / "Cargo.toml").is_file() and (
            candidate / "scripts" / "release"
        ).is_dir():
            return candidate
    raise ReleaseError("could not locate the repository root")


def cargo_toml_path(root: Path) -> Path:
    path = root / "Cargo.toml"
    if not path.is_file():
        raise ReleaseError(f"missing {path}")
    return path


def read_version(root: Path) -> str:
    """Read the ``[package] version`` from ``Cargo.toml``."""
    text = cargo_toml_path(root).read_text(encoding="utf-8")
    package = _PACKAGE_HEADING_RE.search(text)
    if not package:
        raise ReleaseError("Cargo.toml has no [package] section")
    next_section = _NEXT_SECTION_RE.search(text, package.end())
    section = text[package.end() : next_section.start() if next_section else len(text)]
    match = _VERSION_LINE_RE.search(section)
    if not match:
        raise ReleaseError("Cargo.toml has no package version")
    return match.group(1)


def write_version(root: Path, version: str) -> None:
    """Replace the ``[package] version`` in ``Cargo.toml``, preserving layout."""
    path = cargo_toml_path(root)
    text = path.read_text(encoding="utf-8")
    package = _PACKAGE_HEADING_RE.search(text)
    if not package:
        raise ReleaseError("Cargo.toml has no [package] section")
    next_section = _NEXT_SECTION_RE.search(text, package.end())
    end = next_section.start() if next_section else len(text)
    section = text[package.end() : end]
    new_section, count = _VERSION_LINE_RE.subn(
        lambda match: f'version = "{version}"',
        section,
        count=1,
    )
    if count != 1:
        raise ReleaseError("Cargo.toml has no package version")
    path.write_text(text[: package.end()] + new_section + text[end:], encoding="utf-8")


@dataclass(frozen=True)
class Semver:
    major: int
    minor: int
    patch: int
    pre: str = ""

    @classmethod
    def parse(cls, version: str) -> "Semver":
        match = _SEMVER_RE.match(version)
        if not match:
            raise ReleaseError(f"invalid semantic version: {version!r}")
        if match.group("build"):
            raise ReleaseError(
                "versions with build metadata (+...) are not supported by the "
                "release tooling because of download URL encoding"
            )
        return cls(
            major=int(match.group("major")),
            minor=int(match.group("minor")),
            patch=int(match.group("patch")),
            pre=match.group("pre") or "",
        )

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}{self.pre}"

    def bump(self, part: str) -> "Semver":
        if part == "major":
            return Semver(self.major + 1, 0, 0)
        if part == "minor":
            return Semver(self.major, self.minor + 1, 0)
        if part == "patch":
            return Semver(self.major, self.minor, self.patch + 1)
        raise ReleaseError(f"unknown version part: {part!r}")


@dataclass(frozen=True)
class Section:
    title: str
    start: int
    end: int
    body: str


def changelog_path(root: Path) -> Path:
    return root / "CHANGELOG.md"


def read_changelog(root: Path) -> str:
    path = changelog_path(root)
    if not path.is_file():
        raise ReleaseError(f"missing {path}")
    return path.read_text(encoding="utf-8")


def parse_sections(text: str) -> list[Section]:
    matches = list(_CHANGELOG_HEADING_RE.finditer(text))
    sections: list[Section] = []
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        sections.append(
            Section(
                title=match.group(1),
                start=match.start(),
                end=end,
                body=text[match.end() : end],
            )
        )
    return sections


def find_section(text: str, title: str) -> Section:
    for section in parse_sections(text):
        if section.title == title:
            return section
    raise ReleaseError(f"CHANGELOG.md has no section for {title!r}")


def _trim_blank_lines(text: str) -> str:
    return text.strip("\n")


def bump_changelog(
    text: str, version: str, date: str, *, allow_empty: bool = False
) -> str:
    """Move ``[Unreleased]`` entries into a new ``[version] - date`` section."""
    sections = parse_sections(text)
    unreleased = next(
        (section for section in sections if section.title.lower() == "unreleased"),
        None,
    )
    if unreleased is None:
        raise ReleaseError("CHANGELOG.md has no [Unreleased] section")
    if any(section.title == version for section in sections):
        raise ReleaseError(f"CHANGELOG.md already has a section for {version}")

    body = _trim_blank_lines(unreleased.body)
    if not body and not allow_empty:
        raise ReleaseError(
            "the [Unreleased] changelog section is empty; describe the changes "
            "before releasing (or pass --allow-empty)"
        )

    before = text[: unreleased.start]
    after = text[unreleased.end :]
    out = before
    out += "## [Unreleased]\n\n"
    out += f"## [{version}] - {date}\n"
    if body:
        out += "\n" + body + "\n"
    out += "\n"
    if after:
        out += after.lstrip("\n")
    return out


def release_notes(text: str, version: str) -> str:
    body = _trim_blank_lines(find_section(text, version).body)
    if not body:
        raise ReleaseError(f"CHANGELOG.md section for {version} is empty")
    return body + "\n"


def _today() -> str:
    return _dt.datetime.now(_dt.timezone.utc).date().isoformat()


def _cmd_current(args: argparse.Namespace) -> int:
    print(read_version(args.root))
    return 0


def _cmd_bump(args: argparse.Namespace) -> int:
    current = Semver.parse(read_version(args.root))
    new = current.bump(args.part)
    text = bump_changelog(
        read_changelog(args.root),
        str(new),
        args.date,
        allow_empty=args.allow_empty,
    )
    write_version(args.root, str(new))
    changelog_path(args.root).write_text(text, encoding="utf-8")
    print(new)
    return 0


def _cmd_set(args: argparse.Namespace) -> int:
    version = Semver.parse(args.version)
    text = bump_changelog(
        read_changelog(args.root),
        str(version),
        args.date,
        allow_empty=args.allow_empty,
    )
    write_version(args.root, str(version))
    changelog_path(args.root).write_text(text, encoding="utf-8")
    print(version)
    return 0


def _cmd_check(args: argparse.Namespace) -> int:
    tag = args.tag
    if not tag.startswith("v"):
        raise ReleaseError(f"release tags must look like v1.2.3, got {tag!r}")
    tagged = Semver.parse(tag[1:])
    current = read_version(args.root)
    if str(tagged) != current:
        raise ReleaseError(
            f"tag {tag} does not match the Cargo.toml version {current}; "
            "bump the version before tagging"
        )
    body = _trim_blank_lines(find_section(read_changelog(args.root), current).body)
    if not body:
        raise ReleaseError(f"CHANGELOG.md section for {current} is empty")
    print(f"ok: {tag} matches Cargo.toml and CHANGELOG.md")
    return 0


def _cmd_notes(args: argparse.Namespace) -> int:
    notes = release_notes(read_changelog(args.root), args.version)
    if args.out:
        Path(args.out).write_text(notes, encoding="utf-8")
    else:
        sys.stdout.write(notes)
    return 0


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="release.py",
        description="Version, changelog and tag bookkeeping for blogen releases.",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=None,
        help="repository root (defaults to the checkout containing this script)",
    )
    parser.add_argument("--date", default=_today(), help="release date (default: UTC today)")
    subparsers = parser.add_subparsers(dest="command", required=True)

    current = subparsers.add_parser("current", help="print the crate version")
    current.set_defaults(func=_cmd_current)

    bump = subparsers.add_parser("bump", help="bump the version and changelog")
    bump.add_argument("--part", choices=["patch", "minor", "major"], required=True)
    bump.add_argument(
        "--allow-empty",
        action="store_true",
        help="allow a release without changelog entries",
    )
    bump.set_defaults(func=_cmd_bump)

    explicit = subparsers.add_parser("set", help="set an explicit version")
    explicit.add_argument("--version", required=True)
    explicit.add_argument("--allow-empty", action="store_true")
    explicit.set_defaults(func=_cmd_set)

    check = subparsers.add_parser("check", help="validate a release tag")
    check.add_argument("--tag", required=True)
    check.set_defaults(func=_cmd_check)

    notes = subparsers.add_parser("notes", help="print release notes")
    notes.add_argument("--version", required=True)
    notes.add_argument("--out", help="write to a file instead of stdout")
    notes.set_defaults(func=_cmd_notes)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)
    if args.root is None:
        try:
            args.root = repo_root()
        except ReleaseError as error:
            print(f"error: {error}", file=sys.stderr)
            return 1
    try:
        return args.func(args)
    except ReleaseError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
