# Implementation status

This table records implemented dimensions and the evidence required before a
release claim. It is not a substitute for command output or hosted workflow
results.

| Dimension | Current state | Evidence / remaining verification |
| --- | --- | --- |
| Reference and architecture boundaries | Implemented | `FEATURE_PARITY.md`, `ARCHITECTURE.md`, `UX_CONTRACT.md`; `reffer/` remains read-only reference material |
| `vc-core` queue reducer | Implemented and locally verified | Monotonic item sequences, run-bound transitions, execution-profile validation, recovery, and queue invariants are covered by `cargo test --locked -p vc-core` |
| `vc-runtime` planning and execution | Implemented and locally verified | Directory probe isolation, cancellation, worker failure cleanup, and mixed-profile rejection are covered by `cargo test --locked -p vc-runtime` |
| Responsiveness hot paths | Implemented | Debounced window geometry, no UI-thread disk I/O/`block_on`, close timeout + force abort, async file commands, coalesced queue snapshots, single process log writer, bounded activity history, cancellable subscriptions; stress suite in `crates/vc-runtime/tests/stress.rs` |
| React/Tauri UI | Implemented and locally verified | Plan/Add-to-Queue persistence, error handling, queue controls, Preview, Presets, Settings, subscription cleanup, progress-delta reducer, activity batching, and memoized queue rows |
| Hosted CI and parity gates | Verified on implementation commit `7db9bb0` | [CI run 29987707554](https://github.com/starfield17/VideoCompressR/actions/runs/29987707554) and [parity run 29987707553](https://github.com/starfield17/VideoCompressR/actions/runs/29987707553) completed successfully on `main` |
| Packaged Tauri and browser E2E | Verified on implementation commit `7db9bb0` | [Desktop E2E run 29987707590](https://github.com/starfield17/VideoCompressR/actions/runs/29987707590) passed browser Playwright and packaged Tauri WebDriver smoke flows |
| Cross-platform release | v1.1.1 prepared; hosted release gates remain required | [v1.0.0](https://github.com/starfield17/VideoCompressR/releases/tag/v1.0.0); v1.1.1 tags trigger native startup smoke, packaged E2E, and unsigned release artifact publication |

Known limitations: real GPU encoder behavior remains platform-dependent; the
release artifacts are intentionally thin and unsigned; code signing and
notarization are not part of the current release workflow. v1.1.1 remains
unsigned and is not a signed production release under `RELEASE_CONTRACT.md`.
