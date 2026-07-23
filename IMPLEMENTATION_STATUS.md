# Implementation status

This file is updated with the repository rather than used as a promise of
unverified work.

| Phase | Status | Evidence |
| --- | --- | --- |
| 0. Reference audit and contract freeze | Locally verified | `FEATURE_PARITY.md`, `fixtures/legacy/README.md`, `ARCHITECTURE.md`, `UX_CONTRACT.md` |
| 1. Workspace and quality gates | Locally verified; hosted CI pending | `scripts/check_architecture.py`, `cargo deny check`, split CI workflows |
| 2. `vc-core` parity | Locally verified | 10 core tests, bitrate/plan/queue golden fixtures, pure queue reducer |
| 3–8. Runtime, CLI, queue, storage | Locally verified | 20 runtime tests, 10 CLI contract tests, fake tools, direct process runner |
| 9–10. Tauri/React GUI and migration | Implemented; cross-platform CI pending | Tauri 2 build, explicit DTO codegen, 2 Vitest/App smoke tests, WebdriverIO workflow |
| 11–12. Cross-platform release | Implemented; hosted verification pending | Six-target workflow, thin archives, portable checksums, Cargo metadata SBOM, unsigned status manifest |

The authoritative completion evidence is the command output and GitHub Actions
run status in the final report, not this table alone. Linux release packaging
was verified locally; Windows/macOS and ARM targets require their corresponding
hosted runners. Local browser mode is configured but this host lacks Chromium's
`libnspr4.so`; packaged Tauri E2E is configured for CI with `tauri-driver` and
an X server. Hardware encoder coverage uses fake-tool and capability fixtures;
real GPU behavior remains platform-dependent.
