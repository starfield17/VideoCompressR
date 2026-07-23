# Implementation status

This table records implemented dimensions and the evidence required before a
release claim. It is not a substitute for command output or hosted workflow
results.

| Dimension | Current state | Evidence / remaining verification |
| --- | --- | --- |
| Reference and architecture boundaries | Implemented | `FEATURE_PARITY.md`, `ARCHITECTURE.md`, `UX_CONTRACT.md`; `reffer/` remains read-only reference material |
| `vc-core` queue reducer | Implemented | Monotonic item sequences, run-bound transitions, execution-profile validation, and queue invariants are covered by `cargo test --locked -p vc-core` |
| `vc-runtime` planning and execution | Implemented | Directory probe isolation, cancellation, worker failure cleanup, and mixed-profile rejection are covered by `cargo test --locked -p vc-runtime --test runtime` |
| React/Tauri UI | Implemented | Plan/Add-to-Queue persistence, error handling, queue controls, Preview, Presets, Settings, and i18n interaction tests in `apps/desktop/src/App.test.tsx` |
| Hosted CI and parity gates | Baseline green; new reliability pass pending | Baseline: [CI run 29978555373](https://github.com/starfield17/VideoCompressR/actions/runs/29978555373), [parity run 29978555334](https://github.com/starfield17/VideoCompressR/actions/runs/29978555334); the next `main` push must verify the updated workflows |
| Packaged Tauri and browser E2E | Implemented; hosted verification pending | WebDriver flow uses fake tools and an isolated data directory; browser Playwright covers startup and source validation |
| Cross-platform release | Implemented; hosted dry-run pending | Six-target matrix, manual `publish` boolean defaulting to false, concurrency, checksums, decompression, SBOM, licenses, build info, and unsigned-status checks are in `.github/workflows/release.yml` |

Known limitations: real GPU encoder behavior remains platform-dependent; the
release artifacts are intentionally thin and unsigned; Windows/macOS and ARM
claims require their corresponding hosted runners. Completion requires the
full local command matrix plus successful hosted CI, E2E, and a manual
no-publish release run.
