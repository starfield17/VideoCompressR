#!/usr/bin/env python3
"""Optional dependency-free release archive verifier for local development."""

from __future__ import annotations

import argparse
import hashlib
import tarfile
import zipfile
from pathlib import Path


def sha256(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def members(path: Path) -> list[str]:
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            return archive.namelist()
    with tarfile.open(path, "r:gz") as archive:
        return archive.getnames()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    archives = sorted(list(args.directory.glob("*.tar.gz")) + list(args.directory.glob("*.zip")))
    if not archives:
        print("no release archives found")
        return 1
    for archive in archives:
        checksum = archive.with_name(archive.name + ".sha256")
        expected = checksum.read_text(encoding="utf-8").split()[0]
        if expected != sha256(archive):
            print(f"checksum mismatch: {archive}")
            return 1
        names = members(archive)
        for required in ("BUILD_INFO.txt", "SBOM.cargo-metadata.json", "THIRD_PARTY_LICENSES.md"):
            if not any(name.endswith("/" + required) for name in names):
                print(f"missing {required}: {archive}")
                return 1
        print(f"verified {archive.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
