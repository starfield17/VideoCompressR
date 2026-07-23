# Implementation status

This table records implemented dimensions and the evidence required before a
release claim. It is not a substitute for command output or hosted workflow
results.

| Dimension | Current state | Evidence / remaining verification |
| --- | --- | --- |
| Reference and architecture boundaries | Implemented | `FEATURE_PARITY.md`, `ARCHITECTURE.md`, `UX_CONTRACT.md`; `reffer/` remains read-only reference material |
| `vc-core` queue reducer | Implemented and locally verified | Monotonic item sequences, run-bound transitions, execution-profile validation, and queue invariants are covered by `cargo test --locked -p vc-core` (15 passed) |
| `vc-runtime` planning and execution | Implemented and locally verified | Directory probe isolation, cancellation, worker failure cleanup, and mixed-profile rejection are covered by `cargo test --locked -p vc-runtime` (28 passed) |
| React/Tauri UI | Implemented and locally verified | Plan/Add-to-Queue persistence, error handling, queue controls, Preview, Presets, Settings, and i18n interaction tests pass in `pnpm test:run` (18 passed) |
| Hosted CI and parity gates | Verified on implementation commit `7db9bb0` | [CI run 29987707554](https://github.com/starfield17/VideoCompressR/actions/runs/29987707554) and [parity run 29987707553](https://github.com/starfield17/VideoCompressR/actions/runs/29987707553) completed successfully on `main` |
| Packaged Tauri and browser E2E | Verified on implementation commit `7db9bb0` | [Desktop E2E run 29987707590](https://github.com/starfield17/VideoCompressR/actions/runs/29987707590) passed browser Playwright and packaged Tauri WebDriver smoke flows |
| Cross-platform release | v1.0.0 published from `700d163` as an unsigned preview | [Release run 29989250523](https://github.com/starfield17/VideoCompressR/actions/runs/29989250523) passed all 12 CLI/Desktop target jobs and published [GitHub Release v1.0.0](https://github.com/starfield17/VideoCompressR/releases/tag/v1.0.0) with 26 assets |

Known limitations: real GPU encoder behavior remains platform-dependent; the
release artifacts are intentionally thin and unsigned; code signing and
notarization are not part of the current release workflow. v1.0.0 is therefore
an unsigned preview and is not a production release under `RELEASE_CONTRACT.md`.
