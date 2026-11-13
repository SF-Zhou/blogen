#!/usr/bin/env python3
"""Unit tests for the release helper.  Run with ``python3 -m unittest``."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import release  # noqa: E402

CARGO_TOML = """\
[package]
name = "blogen"
version = "0.1.0"
edition = "2021"

[dependencies]
thiserror = { version = "2.0" }
"""

CHANGELOG = """\
# Changelog

## [Unreleased]

### Added

- A shiny new feature.

## [0.1.0] - 2026-11-13

### Added

- Initial release.
"""


class ReleaseTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "Cargo.toml").write_text(CARGO_TOML, encoding="utf-8")
        (self.root / "CHANGELOG.md").write_text(CHANGELOG, encoding="utf-8")
        # The module locates the repository root from its own path; tests pass
        # the temporary root explicitly instead.
        self.args = argparse_namespace(self.root)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def version(self) -> str:
        return release.read_version(self.root)

    def changelog(self) -> str:
        return release.read_changelog(self.root)

    def test_read_version(self) -> None:
        self.assertEqual(self.version(), "0.1.0")

    def test_bump_patch_moves_changelog_entries(self) -> None:
        self.assertEqual(release.main([*self.args, "bump", "--part", "patch"]), 0)
        self.assertEqual(self.version(), "0.1.1")
        text = self.changelog()
        self.assertIn("## [0.1.1] -", text)
        self.assertLess(text.index("## [Unreleased]"), text.index("## [0.1.1]"))
        self.assertLess(text.index("## [0.1.1]"), text.index("## [0.1.0]"))
        self.assertIn("A shiny new feature", text)
        self.assertEqual(text.count("A shiny new feature"), 1)

    def test_bump_minor_and_major(self) -> None:
        self.assertEqual(release.main([*self.args, "bump", "--part", "minor"]), 0)
        self.assertEqual(self.version(), "0.2.0")
        (self.root / "CHANGELOG.md").write_text(CHANGELOG, encoding="utf-8")
        self.assertEqual(release.main([*self.args, "bump", "--part", "major"]), 0)
        self.assertEqual(self.version(), "1.0.0")

    def test_bump_only_touches_package_version(self) -> None:
        release.main([*self.args, "bump", "--part", "minor"])
        text = (self.root / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('version = "0.2.0"', text)
        self.assertIn('thiserror = { version = "2.0" }', text)

    def test_empty_unreleased_is_rejected(self) -> None:
        (self.root / "CHANGELOG.md").write_text(
            "# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-11-13\n\n- old\n",
            encoding="utf-8",
        )
        self.assertEqual(release.main([*self.args, "bump", "--part", "patch"]), 1)
        self.assertEqual(self.version(), "0.1.0")

    def test_explicit_prerelease_version(self) -> None:
        self.assertEqual(
            release.main([*self.args, "set", "--version", "0.2.0-rc.1"]), 0
        )
        self.assertEqual(self.version(), "0.2.0-rc.1")
        self.assertIn("## [0.2.0-rc.1] -", self.changelog())

    def test_build_metadata_is_rejected(self) -> None:
        self.assertEqual(
            release.main([*self.args, "set", "--version", "0.2.0+build.5"]), 1
        )

    def test_notes_extract_section(self) -> None:
        out = self.root / "notes.md"
        self.assertEqual(
            release.main(
                [*self.args, "notes", "--version", "0.1.0", "--out", str(out)]
            ),
            0,
        )
        notes = out.read_text(encoding="utf-8")
        self.assertIn("Initial release", notes)
        self.assertNotIn("Shiny", notes)
        self.assertNotIn("## [0.1.0]", notes)

    def test_check_accepts_matching_tag(self) -> None:
        self.assertEqual(release.main([*self.args, "check", "--tag", "v0.1.0"]), 0)

    def test_check_rejects_mismatched_tag(self) -> None:
        self.assertEqual(release.main([*self.args, "check", "--tag", "v0.2.0"]), 1)

    def test_check_rejects_missing_changelog_section(self) -> None:
        (self.root / "Cargo.toml").write_text(
            CARGO_TOML.replace('version = "0.1.0"', 'version = "0.3.0"'),
            encoding="utf-8",
        )
        self.assertEqual(release.main([*self.args, "check", "--tag", "v0.3.0"]), 1)

    def test_invalid_version_in_cargo_toml(self) -> None:
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "blogen"\nversion = "not-a-version"\n',
            encoding="utf-8",
        )
        self.assertEqual(release.main([*self.args, "bump", "--part", "patch"]), 1)


def argparse_namespace(root: Path) -> list[str]:
    return ["--root", str(root)]


if __name__ == "__main__":
    unittest.main()
