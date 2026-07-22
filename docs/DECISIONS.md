# Decisions

## D-0001: Preserve legacy JSON names

The reference presets use `min_video_kbps`, `max_video_kbps`, `preset`, and
snake_case fields. Rust keeps those names at the storage boundary and accepts
missing fields introduced by later versions. New schema metadata is additive.

## D-0002: Preserve legacy discovery behavior during migration

The reference tests allow an explicit FFmpeg path to be paired with an
automatically discovered FFprobe. Runtime discovery therefore keeps that
behavior while preferring complete bundled pairs.

## D-0003: Queue state is reduced centrally

The reference GUI mutates records in several places. The rewrite centralizes
legal transitions in `vc-core::queue::apply`; runtime workers report commands
with a run identifier so stale progress cannot update a retry.

## D-0004: Thin release first

No unverified third-party binary is committed or downloaded. Full FFmpeg
artifacts remain conditional on a checked provenance manifest.

## D-0005: Native process-tree ownership per platform

FFmpeg runs without a shell. Unix children are placed in a process group;
Windows children are assigned to a Job Object with kill-on-close semantics.
Cancellation first offers FFmpeg graceful input, then applies the platform
tree cleanup path.

## D-0006: Generated IPC bindings

Tauri DTOs derive `ts-rs` declarations during the Rust build. The React API
client imports the generated file, so command payload changes fail at the
Rust/TypeScript boundary instead of relying on duplicated handwritten types.
