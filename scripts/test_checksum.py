#!/usr/bin/env python3
"""Smoke test for the portable checksum helper."""

from __future__ import annotations

import tempfile
from pathlib import Path

from checksum import create, verify


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="video-compressor-checksum-") as directory:
        archive = Path(directory) / "archive.tar.gz"
        archive.write_bytes((b"VideoCompressR\n" * 200_000) + b"end\n")
        checksum = create(archive)
        assert checksum.read_text(encoding="utf-8").endswith("  archive.tar.gz\n")
        assert verify(checksum) == archive
        archive.write_bytes(b"tampered")
        try:
            verify(checksum)
        except ValueError:
            pass
        else:
            raise AssertionError("tampered archive was accepted")
    print("checksum helper smoke test passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
