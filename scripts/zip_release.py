#!/usr/bin/env python3
"""Create and extract release ZIP archives without external archive tools."""

from __future__ import annotations

import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


def create(archive: Path, root: Path, name: str) -> None:
    source = root / name
    with ZipFile(archive, "w", ZIP_DEFLATED) as output:
        for path in sorted(source.rglob("*")):
            if path.is_file():
                output.write(path, path.relative_to(root))


def extract(archive: Path, destination: Path) -> None:
    with ZipFile(archive) as source:
        for member in source.infolist():
            member_path = (destination / member.filename).resolve()
            if destination.resolve() not in member_path.parents:
                raise ValueError(f"ZIP member escapes extraction directory: {member.filename}")
        source.extractall(destination)
        for member in source.infolist():
            print(member.filename)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    create_parser = subparsers.add_parser("create")
    create_parser.add_argument("archive", type=Path)
    create_parser.add_argument("root", type=Path)
    create_parser.add_argument("name")

    extract_parser = subparsers.add_parser("extract")
    extract_parser.add_argument("archive", type=Path)
    extract_parser.add_argument("destination", type=Path)

    args = parser.parse_args()
    if args.command == "create":
        create(args.archive, args.root, args.name)
    else:
        extract(args.archive, args.destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
