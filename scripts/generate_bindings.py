#!/usr/bin/env python3
"""Run or verify the explicit Rust/ts-rs binding generator."""

from __future__ import annotations

import argparse
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GENERATED = ROOT / "apps" / "desktop" / "src" / "api" / "generated.ts"
COMMAND = [
    "cargo",
    "run",
    "--locked",
    "-p",
    "video-compressor-desktop",
    "--bin",
    "generate-bindings",
    "--",
]


def canonicalize(value: bytes) -> bytes:
    return value.replace(b"\r\n", b"\n")


def run_generator(output: Path) -> int:
    result = subprocess.run(
        [*COMMAND, "--output", str(output)],
        cwd=ROOT,
        check=False,
    )
    return result.returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Build and verify the generated declaration exists")
    args = parser.parse_args()
    if not args.check:
        return run_generator(GENERATED)
    if not GENERATED.is_file():
        print(f"missing generated bindings: {GENERATED}")
        return 1
    with tempfile.TemporaryDirectory(prefix="video-compressor-codegen-") as directory:
        candidate = Path(directory) / "generated.ts"
        if run_generator(candidate) != 0:
            return 1
        expected = GENERATED.read_bytes()
        actual = candidate.read_bytes()
        if canonicalize(expected) != canonicalize(actual):
            print(f"generated bindings are stale: {GENERATED}; run `pnpm codegen`", flush=True)
            return 1
    print(f"verified generated bindings: {GENERATED}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
