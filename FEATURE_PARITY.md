# Feature parity matrix

| Reference area | Legacy evidence | Rust target | Fixture/test | Status |
| --- | --- | --- | --- | --- |
| Models/defaults | `core/models.py`, default presets | `vc-core::model` | `fixtures/legacy/presets` | Complete |
| Bitrate policy | `core/bitrate_policy.py`, quality tests | `vc-core::planning::bitrate` | core table tests + golden fixture | Complete |
| Encoder selection | `core/encoder_caps.py`, capability tests | core rules + runtime capabilities | capability cache and planner tests | Complete |
| Output/safety | `path_utils.py`, `safety_checks.py` | core output rules + runtime filesystem | core planner tests | Complete |
| FFprobe/scanning | `probe_media.py`, `scan_videos.py` | runtime ffprobe/scanner | fake-tool tests | Complete |
| FFmpeg argv/progress | `build_ffmpeg_cmd.py`, `exec_encode.py` | typed commands/process runner | fake capture/stream/failure/cancel tests | Complete |
| Presets/config/i18n | `preset_store.py`, `config/` | runtime JSON stores + resources | copied fixtures + corrupt recovery | Complete |
| Preview | `preview_sample.py`, `preview_estimate.py` | core model + runtime service | runtime preview path | Complete |
| Queue | `gui/queue_state.py`, `queue_manager.py`, parallel tests | core reducer + runtime supervisor | stale-run, serial, parallel controls | Complete |
| CLI | `cli/cli_entry.py`, `cli_interactive.py` | `apps/cli` | help/contract fixtures | Complete |
| GUI | `gui_mainwindow.py` and auxiliary windows | Tauri/React | `UX_CONTRACT.md`, 2 Vitest tests, browser smoke, WebdriverIO workflow | Implemented; local browser/desktop E2E environment pending |
| Release | legacy workflow tests and architecture handout | Thin six-target workflow | `RELEASE_CONTRACT.md`, package verifier | Implemented; hosted matrix pending; artifacts explicitly unsigned |

Reference files remain under `reffer/` and are never required by root builds.
