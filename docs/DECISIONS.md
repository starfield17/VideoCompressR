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

Tauri DTOs derive `ts-rs` declarations through the explicit `pnpm codegen`
command. The React API client imports the checked-in generated file, and
`pnpm codegen:check` compares a temporary regeneration without mutating the
checkout, so command payload changes fail at the Rust/TypeScript boundary
instead of relying on duplicated handwritten types.

## D-0007: Queue invariants are validated at reducer boundaries

Queue state is validated after every reducer command. Commands operate on a
structural clone and replace the state only after validation; the high-volume
progress command updates in place and validates immediately. This keeps
invalid lifecycle combinations out of published snapshots without adding a
second state model or a speculative event-sourcing layer.

## D-0008: A queue run has one normalized execution profile

`StartRun` accepts only queued items with one normalized profile: either all
serial or all parallel with the same deduplicated explicit backend set.
Validation happens before the run identifier is assigned, so a rejected mixed
queue cannot partially start workers. Parallel scheduling consumes the
validated profile rather than re-deriving it from whichever item a worker sees
first.
