#!/usr/bin/env python3
"""Create and verify portable SHA-256 checksum files."""

from __future__ import annotations

import argparse
import hashlib
import re
from pathlib import Path


CHUNK_SIZE = 1024 * 1024
HEX_DIGEST = re.compile(r"^[0-9a-f]{64}$")


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(CHUNK_SIZE), b""):
            value.update(chunk)
    return value.hexdigest()


def create(archive: Path) -> Path:
    checksum = archive.with_name(archive.name + ".sha256")
    checksum.write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")
    return checksum


def verify(checksum: Path) -> Path:
    fields = checksum.read_text(encoding="utf-8").strip().split(maxsplit=1)
    if len(fields) != 2 or not HEX_DIGEST.fullmatch(fields[0]):
        raise ValueError(f"invalid checksum record: {checksum}")
    filename = fields[1].strip()
    if Path(filename).name != filename:
        raise ValueError(f"checksum contains a non-local archive path: {filename}")
    archive = checksum.parent / filename
    if not archive.is_file():
        raise FileNotFoundError(archive)
    actual = digest(archive)
    if actual != fields[0]:
        raise ValueError(f"checksum mismatch for {archive}: expected {fields[0]}, got {actual}")
    return archive


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    create_parser = subparsers.add_parser("create")
    create_parser.add_argument("archive", type=Path)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("checksum", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "create":
            checksum = create(args.archive)
            print(f"created {checksum}")
        else:
            archive = verify(args.checksum)
            print(f"verified {archive}")
    except (OSError, ValueError) as error:
        print(f"checksum error: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
