#!/usr/bin/env python3
"""Run the checked-in Rust/ts-rs binding pipeline for local development.

`apps/desktop/src-tauri/build.rs` is authoritative.  This helper is only a
convenience wrapper; CI and release invoke Cargo directly so Python is not a
final build dependency.
"""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GENERATED = ROOT / "apps" / "desktop" / "src" / "api" / "generated.ts"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Build and verify the generated declaration exists")
    args = parser.parse_args()
    command = ["cargo", "check", "--locked", "-p", "video-compressor-desktop"]
    result = subprocess.run(command, cwd=ROOT, check=False)
    if result.returncode != 0:
        return result.returncode
    if not GENERATED.is_file() or "@generated" not in GENERATED.read_text(encoding="utf-8"):
        print(f"missing generated bindings: {GENERATED}")
        return 1
    print(f"verified generated bindings: {GENERATED}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
