#!/usr/bin/env python3
"""Build a release archive for one target.

Creates ``blogen-<version>-<target>.tar.gz`` in the output directory. The
archive contains the executable at its root so that the installer and
``cargo-binstall`` can find it without knowing about a wrapper directory.

Entries use a fixed timestamp (``SOURCE_DATE_EPOCH`` when set) and the gzip
header timestamp is zeroed, which makes archives reproducible for a given
binary.
"""

from __future__ import annotations

import argparse
import gzip
import io
import os
import subprocess
import sys
import tarfile
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from release import ReleaseError, Semver, read_version, repo_root  # noqa: E402

GENERATED = Path("<generated>")


@dataclass(frozen=True)
class Entry:
    """One file inside the archive."""

    name: str
    path: Path
    mode: int


def _run(*command: str) -> str | None:
    try:
        return subprocess.run(
            command, check=True, capture_output=True, text=True
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def detect_host_target() -> str:
    output = _run("rustc", "-vV")
    if output:
        for line in output.splitlines():
            if line.startswith("host: "):
                return line[len("host: ") :]
    raise ReleaseError("cannot detect the host target; pass --target explicitly")


def detect_rustc() -> str:
    return _run("rustc", "--version") or "unknown"


def detect_commit(root: Path) -> str:
    return _run("git", "-C", str(root), "rev-parse", "--short=12", "HEAD") or "unknown"


def source_date_epoch(root: Path) -> int:
    value = os.environ.get("SOURCE_DATE_EPOCH")
    if value:
        try:
            return int(value)
        except ValueError as error:
            raise ReleaseError(f"invalid SOURCE_DATE_EPOCH: {value!r}") from error
    commit_time = _run("git", "-C", str(root), "log", "-1", "--format=%ct")
    if commit_time:
        return int(commit_time)
    return int(time.time())


def find_binary(root: Path, target: str, profile: str) -> Path:
    candidates = [
        root / "target" / target / profile / "blogen",
        root / "target" / profile / "blogen",
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    searched = "\n  ".join(str(candidate) for candidate in candidates)
    raise ReleaseError(f"binary not found (build it first); looked in:\n  {searched}")


def build_info(version: str, target: str, commit: str, rustc: str, epoch: int) -> str:
    built = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(epoch))
    return (
        f"blogen {version}\n"
        f"target: {target}\n"
        f"commit: {commit}\n"
        f"rustc: {rustc}\n"
        f"built: {built}\n"
    )


def collect_entries(root: Path, binary: Path) -> list[Entry]:
    entries = [
        Entry("blogen", binary, 0o755),
        Entry("BUILD_INFO.txt", GENERATED, 0o644),
    ]
    for name in ("README.md", "CHANGELOG.md", "LICENSE-MIT", "LICENSE-APACHE"):
        path = root / name
        if path.is_file():
            entries.append(Entry(name, path, 0o644))
    return sorted(entries, key=lambda entry: entry.name)


def entry_bytes(entry: Entry, info: str) -> bytes:
    return info.encode("utf-8") if entry.path == GENERATED else entry.path.read_bytes()


def write_tar_gz(archive: Path, entries: list[Entry], info: str, epoch: int) -> None:
    with archive.open("wb") as raw:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9
        ) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tar:
                for entry in entries:
                    data = entry_bytes(entry, info)
                    header = tarfile.TarInfo(entry.name)
                    header.size = len(data)
                    header.mode = entry.mode
                    header.mtime = epoch
                    header.uid = 0
                    header.gid = 0
                    header.uname = ""
                    header.gname = ""
                    tar.addfile(header, io.BytesIO(data))


def verify_archive(archive: Path) -> None:
    with tarfile.open(archive, "r:gz") as tar:
        names = tar.getnames()
    if "blogen" not in names:
        raise ReleaseError(f"{archive} does not contain blogen")
    missing = {"BUILD_INFO.txt", "README.md"} - set(names)
    if missing:
        raise ReleaseError(f"{archive} is missing {sorted(missing)}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=None, help="Rust target triple")
    parser.add_argument("--version", default=None, help="package version")
    parser.add_argument("--profile", default="dist", help="cargo build profile")
    parser.add_argument("--out", default="dist", help="output directory")
    parser.add_argument("--binary", default=None, help="explicit binary path")
    parser.add_argument("--source-date-epoch", type=int, default=None)
    args = parser.parse_args(argv)

    try:
        root = repo_root()
        version = args.version or read_version(root)
        try:
            Semver.parse(version)
        except ReleaseError as error:
            raise ReleaseError(f"invalid --version: {error}") from error
        target = args.target or detect_host_target()
        if "windows" in target:
            raise ReleaseError("Windows targets are not supported")
        epoch = (
            args.source_date_epoch
            if args.source_date_epoch is not None
            else source_date_epoch(root)
        )
        binary = (
            Path(args.binary) if args.binary else find_binary(root, target, args.profile)
        )
        if not binary.is_file():
            raise ReleaseError(f"binary does not exist: {binary}")

        out_dir = Path(args.out)
        if not out_dir.is_absolute():
            out_dir = root / out_dir
        out_dir.mkdir(parents=True, exist_ok=True)

        entries = collect_entries(root, binary)
        info = build_info(
            version,
            target,
            detect_commit(root),
            detect_rustc(),
            epoch,
        )
        base = out_dir / f"blogen-{version}-{target}"

        archive = base.with_name(base.name + ".tar.gz")
        write_tar_gz(archive, entries, info, epoch)
        verify_archive(archive)
        print(archive)
        return 0
    except ReleaseError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
