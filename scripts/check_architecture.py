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


def function_body(source: str, name: str) -> str:
    match = re.search(rf"\bfn\s+{re.escape(name)}\b[^{{]*\{{", source)
    if not match:
        fail(f"cannot find Rust function {name}")
    depth = 0
    for index in range(match.end() - 1, len(source)):
        character = source[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return source[match.end():index]
    fail(f"cannot parse Rust function {name}")
    raise AssertionError


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

    runtime_manifest = read(ROOT / "crates/vc-runtime/Cargo.toml")
    if re.search(r"^\s*tauri\s*=", runtime_manifest, re.MULTILINE):
        fail("vc-runtime depends on Tauri")

    supervisor_source = read(ROOT / "crates/vc-runtime/src/queue/supervisor.rs")
    supervisor_constructor = function_body(supervisor_source, "new")
    if re.search(r"tokio::spawn|spawn_|tokio::time::sleep|\bsleep\s*\(", supervisor_constructor):
        fail("QueueSupervisor::new starts or schedules background work")
    force_abort = function_body(supervisor_source, "force_abort_active_run")
    if "QueueCommand::RecoverRun" not in force_abort:
        fail("force_abort_active_run must use the queue reducer recovery command")
    if re.search(r"inner\.state|run_state|active_run_id|\.status\s*=", force_abort):
        fail("force_abort_active_run writes queue state outside the reducer")

    application_source = read(ROOT / "crates/vc-runtime/src/application.rs")
    bootstrap = function_body(application_source, "bootstrap")
    if re.search(r"tokio::spawn|tokio::runtime|Handle::(?:current|try_current)", bootstrap):
        fail("Application::bootstrap requires or creates a Tokio runtime")

    process_source = read(ROOT / "crates/vc-runtime/src/ffmpeg/process.rs")
    capture_exact = function_body(process_source, "run_capture_exact")
    if "try_send" in capture_exact:
        fail("run_capture_exact may not drop output through try_send")

    tauri_text = "\n".join(read(path) for path in rust_sources(ROOT / "apps/desktop/src-tauri/src"))
    for forbidden in ("Command::new", "render_encode_commands", "-progress", "-c:v"):
        if forbidden in tauri_text:
            fail(f"Tauri adapter contains runtime implementation detail: {forbidden}")

    # UI-thread / responsiveness guards for the desktop adapter.
    tauri_lib = ROOT / "apps/desktop/src-tauri/src/lib.rs"
    if tauri_lib.exists():
        lib_text = read(tauri_lib)
        # Strip unit-test module so architectural assertions target production code.
        production = re.split(r"#\[cfg\(test\)\]\s*mod tests", lib_text, maxsplit=1)[0]
        handler_start = production.find(".on_window_event")
        if handler_start < 0:
            fail("desktop adapter missing on_window_event handler")
        handler_end = production.find(".invoke_handler", handler_start)
        handler = production[handler_start:handler_end if handler_end > 0 else None]
        for forbidden in ("block_on(", "block_in_place", "sync_all", "std::fs::"):
            if forbidden in handler:
                fail(f"window event handler must not use {forbidden}")
        if "store.save" in handler or "window_state.save" in handler:
            fail("window event handler must not save window state on the UI thread")
        if "snapshot_now" not in handler:
            fail("window close path must use queue.snapshot_now() (no block_on)")
        geometry_callback = function_body(lib_text, "note_geometry")
        if re.search(r"tokio::spawn|async_runtime::spawn|tokio::time::sleep|\bsleep\s*\(", geometry_callback):
            fail("geometry event handling must only note the latest geometry")
        for name in (
            "save_settings",
            "save_app_settings",
            "preset_list",
            "preset_load",
            "preset_save",
            "preset_delete",
            "activity_history",
            "activity_export",
            "open_aux_window",
        ):
            if f"async fn {name}" not in production:
                fail(f"file I/O command must be async: {name}")

    process_text = "\n".join(
        read(path) for path in rust_sources(ROOT / "crates/vc-runtime/src/ffmpeg")
    )
    if "unbounded_channel" in process_text:
        fail("process output must not use unbounded_channel")

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

    frontend_app = read(ROOT / "apps/desktop/src/App.tsx")
    if re.search(r"ActivityRow[\s\S]{0,500}key={[^}]*index", frontend_app):
        fail("Activity rows must use a monotonic event identity, not an array index")

    release_workflow = read(ROOT / ".github/workflows/release.yml")
    for job in ("desktop-startup-smoke:", "packaged-e2e:"):
        if job not in release_workflow:
            fail(f"release workflow is missing {job}")
    if "pnpm tauri build --ci" not in release_workflow:
        fail("release workflow startup smoke must build the native desktop binary")
    if "pnpm --dir apps/desktop e2e" not in release_workflow:
        fail("release workflow is missing packaged desktop E2E")
    if "needs: [cli, desktop, desktop-startup-smoke, packaged-e2e]" not in release_workflow:
        fail("release publication must wait for build, startup smoke, and packaged E2E gates")

    production_sources = (
        rust_sources(ROOT / "crates/vc-core/src")
        + rust_sources(ROOT / "crates/vc-runtime/src")
        + [ROOT / "apps/cli/src/main.rs"]
        + rust_sources(ROOT / "apps/desktop/src-tauri/src")
    )
    for path in production_sources:
        # Unit-test modules inside src/ are allowed to use unwrap/expect.
        production = re.split(r"#\[cfg\(test\)\]\s*mod\s+\w+", read(path), maxsplit=1)[0]
        if re.search(r"\.(unwrap|expect)\s*\(", production):
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
