#!/usr/bin/env python3
"""Write a ``SHA256SUMS`` file (``sha256sum`` format) for a directory."""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", default="dist")
    parser.add_argument("--output", default=None, help="defaults to DIR/SHA256SUMS")
    args = parser.parse_args(argv)

    directory = Path(args.directory)
    if not directory.is_dir():
        print(f"error: {directory} is not a directory", file=sys.stderr)
        return 1
    output = Path(args.output) if args.output else directory / "SHA256SUMS"

    files = sorted(
        path
        for path in directory.iterdir()
        if path.is_file() and path.name != output.name and not path.name.endswith(".sha256")
    )
    if not files:
        print(f"error: no files to checksum in {directory}", file=sys.stderr)
        return 1
    lines = [f"{sha256(path)}  {path.name}\n" for path in files]
    with output.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("".join(lines))
    print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
