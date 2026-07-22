#!/usr/bin/env python3
"""Capture deterministic migration inputs from the read-only Python reference.

This helper is intentionally a development/migration tool.  It never imports
the reference GUI and never writes below the reference tree.  The final Rust
build, tests, and release workflows use only the copied files under
``fixtures/legacy``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REFERENCE = ROOT / "reffer" / "code" / "Video_compress_Encoder_gui"
DEFAULT_OUTPUT = ROOT / "fixtures" / "legacy"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def copy_reference_inputs(reference: Path, output: Path) -> list[str]:
    copied: list[str] = []
    mappings = {
        reference / "config" / "i18n": output / "config",
        reference / "config" / "presets": output / "presets",
    }
    for source_dir, target_dir in mappings.items():
        if not source_dir.is_dir():
            continue
        target_dir.mkdir(parents=True, exist_ok=True)
        for source in sorted(source_dir.glob("*.json")):
            target = target_dir / source.name
            shutil.copy2(source, target)
            copied.append(str(target.relative_to(output)))
    return copied


def source_manifest(reference: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(reference.rglob("*")):
        if not path.is_file() or ".git" in path.parts:
            continue
        entries.append(
            {
                "path": path.relative_to(reference).as_posix(),
                "bytes": path.stat().st_size,
                "sha256": sha256(path),
            }
        )
    return entries


def gui_matrix(reference: Path) -> dict[str, Any]:
    windows: list[dict[str, Any]] = []
    actions: set[str] = set()
    controls: set[str] = set()
    for path in sorted((reference / "gui").glob("*.py")):
        text = path.read_text(encoding="utf-8", errors="replace")
        classes = re.findall(r"^class\s+(\w+).*?\(([^)]*)\):", text, re.MULTILINE)
        for name, bases in classes:
            if any(token in bases for token in ("QMainWindow", "QDialog", "QWidget")):
                windows.append({"module": path.name, "class": name, "bases": bases})
        actions.update(re.findall(r"(?:QAction|QPushButton)\([^\n]*", text))
        controls.update(re.findall(r"self\.([A-Za-z_]\w*)\s*=\s*(?:Q|self\.)", text))
    return {
        "windows": windows,
        "action_source_lines": sorted(actions),
        "named_controls": sorted(controls),
        "i18n_keys": sorted(
            set(
                re.findall(
                    r"(?:tr|self\.tr)\.t\(\s*[\"']([^\"']+)",
                    "\n".join(
                        path.read_text(encoding="utf-8", errors="replace")
                        for path in sorted((reference / "gui").glob("*.py"))
                    ),
                )
            )
        ),
    }


def capture_cli_help(reference: Path, output: Path) -> dict[str, Any]:
    code = "from cli.cli_entry import _build_parser; print(_build_parser().format_help(), end='')"
    environment = os.environ.copy()
    environment["PYTHONPATH"] = str(reference) + os.pathsep + environment.get("PYTHONPATH", "")
    try:
        result = subprocess.run(
            [sys.executable, "-c", code],
            cwd=reference,
            env=environment,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=20,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        status = {"captured": False, "returncode": None, "error": str(error)}
        write_json(output / "cli" / "capture-status.json", status)
        return status

    (output / "cli").mkdir(parents=True, exist_ok=True)
    (output / "cli" / "help.txt").write_text(result.stdout, encoding="utf-8")
    status = {
        "captured": result.returncode == 0,
        "returncode": result.returncode,
        "stderr": result.stderr,
    }
    write_json(output / "cli" / "capture-status.json", status)
    return status


def ensure_contract_directories(output: Path) -> None:
    readmes = {
        "plans": "Plan fixtures are normalized EncodePlan snapshots captured from the legacy implementation.\n",
        "commands": "Command fixtures contain normalized FFmpeg argv and progress protocol observations.\n",
        "capabilities": "Capability fixtures contain normalized encoder/hwaccel cache observations.\n",
        "queue": "Queue fixtures contain reducer scenarios and expected state transitions.\n",
        "screenshots": "GUI screenshots are optional when the host has no display; UX_CONTRACT.md records the code-derived matrix.\n",
    }
    for name, text in readmes.items():
        path = output / name / "README.md"
        if not path.exists():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-root", type=Path, default=DEFAULT_REFERENCE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--no-copy", action="store_true", help="Only inspect the reference tree")
    parser.add_argument("--strict", action="store_true", help="Fail when legacy CLI capture cannot run")
    args = parser.parse_args()

    reference = args.reference_root.expanduser().resolve()
    output = args.output.expanduser().resolve()
    if not reference.is_dir():
        print(f"reference root does not exist: {reference}", file=sys.stderr)
        return 2
    if not args.no_copy:
        copied = copy_reference_inputs(reference, output)
        ensure_contract_directories(output)
    else:
        copied = []

    write_json(output / "source-manifest.json", {"reference_root": "read-only", "files": source_manifest(reference)})
    write_json(output / "gui-matrix.json", gui_matrix(reference))
    cli_status = capture_cli_help(reference, output)
    if args.strict and not cli_status.get("captured", False):
        return 1
    print(f"captured {len(copied)} copied JSON inputs and {len(source_manifest(reference))} reference files")
    if not cli_status.get("captured", False):
        print("legacy CLI help was not captured; see fixtures/legacy/cli/capture-status.json", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
