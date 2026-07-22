#!/usr/bin/env python3
"""Small, dependency-free architecture guard for the root rewrite."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"architecture check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"cannot read {path}: {error}")
    raise AssertionError


def rust_sources(directory: Path) -> list[Path]:
    return sorted(directory.rglob("*.rs")) if directory.exists() else []


def main() -> int:
    core_manifest = read(ROOT / "crates/vc-core/Cargo.toml")
    forbidden_core_dependencies = ("tokio", "tauri", "clap", "directories")
    for dependency in forbidden_core_dependencies:
        if re.search(rf"^\s*{re.escape(dependency)}\s*=", core_manifest, re.MULTILINE):
            fail(f"vc-core depends on {dependency}")

    core_text = "\n".join(read(path) for path in rust_sources(ROOT / "crates/vc-core/src"))
    for pattern, reason in (
        (r"\btokio\b", "tokio in vc-core source"),
        (r"std::process|Command::new", "process spawning in vc-core source"),
        (r"std::fs|directories::", "filesystem/application paths in vc-core source"),
    ):
        if re.search(pattern, core_text):
            fail(reason)

    tauri_text = "\n".join(read(path) for path in rust_sources(ROOT / "apps/desktop/src-tauri/src"))
    for forbidden in ("Command::new", "render_encode_commands", "-progress", "-c:v"):
        if forbidden in tauri_text:
            fail(f"Tauri adapter contains runtime implementation detail: {forbidden}")

    cli_text = read(ROOT / "apps/cli/src/main.rs")
    if "resolve_encoder" in cli_text or "encoder_candidates" in cli_text:
        fail("CLI contains encoder-selection rules")

    frontend_root = ROOT / "apps/desktop/src"
    for path in sorted(frontend_root.rglob("*.ts")) + sorted(frontend_root.rglob("*.tsx")):
        if "src/api" in path.as_posix() or path.name == "generated.ts":
            continue
        if re.search(r"\binvoke\s*\(", read(path)):
            fail(f"frontend invokes Tauri outside src/api: {path}")

    capabilities = read(ROOT / "apps/desktop/src-tauri/capabilities/default.json")
    if re.search(r"shell|spawn", capabilities, re.IGNORECASE):
        fail("desktop capability grants shell/spawn access")

    generated = read(ROOT / "apps/desktop/src/api/generated.ts")
    if "@generated" not in generated:
        fail("TypeScript DTO bindings are not marked generated")

    production_sources = (
        rust_sources(ROOT / "crates/vc-core/src")
        + rust_sources(ROOT / "crates/vc-runtime/src")
        + [ROOT / "apps/cli/src/main.rs"]
        + rust_sources(ROOT / "apps/desktop/src-tauri/src")
    )
    for path in production_sources:
        if re.search(r"\.(unwrap|expect)\s*\(", read(path)):
            fail(f"production Rust source uses unwrap/expect: {path}")

    root_build_inputs = [ROOT / "Cargo.toml", ROOT / "pnpm-workspace.yaml", ROOT / "package.json"]
    root_build_inputs += list((ROOT / ".github").rglob("*")) if (ROOT / ".github").exists() else []
    for path in root_build_inputs:
        if path.is_file() and "reffer/" in read(path):
            fail(f"root build input depends on reffer: {path}")

    print("architecture check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
