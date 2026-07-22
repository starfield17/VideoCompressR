#!/usr/bin/env python3
"""Stage a provenance-checked local FFmpeg/FFprobe pair for a Full build.

The script deliberately has no network code.  A caller must provide local
files, target identity, expected SHA-256 values, version, and license/source
metadata in the manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path
from typing import Any


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def fail(message: str) -> None:
    raise SystemExit(f"staging failed: {message}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()

    manifest_path = args.manifest.expanduser().resolve()
    manifest: dict[str, Any] = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("target") != args.target:
        fail(f"manifest target {manifest.get('target')!r} does not match {args.target!r}")
    for required in ("version", "source", "license", "ffmpeg", "ffprobe"):
        if required not in manifest:
            fail(f"manifest is missing {required}")
    if not manifest["source"].get("url") or not manifest["source"].get("revision"):
        fail("source.url and source.revision are required provenance")
    if not manifest["license"].get("name") or not manifest["license"].get("url"):
        fail("license.name and license.url are required provenance")

    output = args.output.expanduser().resolve()
    bin_dir = output / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if "windows" in args.target else ""
    staged: dict[str, dict[str, str]] = {}
    for name in ("ffmpeg", "ffprobe"):
        entry = manifest[name]
        source = Path(str(entry.get("path", ""))).expanduser().resolve()
        if not source.is_file():
            fail(f"{name} source is not a file: {source}")
        actual = digest(source)
        expected = str(entry.get("sha256", "")).lower()
        if len(expected) != 64 or actual != expected:
            fail(f"{name} SHA-256 mismatch: expected {expected}, got {actual}")
        target = bin_dir / f"{name}{suffix}"
        shutil.copy2(source, target)
        staged[name] = {"file": target.name, "sha256": actual}

    provenance = {
        "target": args.target,
        "version": manifest["version"],
        "source": manifest["source"],
        "license": manifest["license"],
        "staged": staged,
        "verified": True,
    }
    (output / "FFMPEG_PROVENANCE.json").write_text(
        json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"staged verified FFmpeg pair for {args.target} in {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
