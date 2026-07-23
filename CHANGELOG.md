# Changelog

## v1.1.1 - 2026-07-23

- Fixes startup lifecycle initialization so queue workers are created only inside an existing async runtime and shut down cleanly.
- Preserves complete FFmpeg/FFprobe stdout and stderr, including backpressure, invalid UTF-8, and reader errors.
- Bounds geometry debounce work and recovers Activity subscribers after broadcast lag.
- Recovers interrupted queue runs through the core reducer and surfaces process-log open/write/finish failures.
- Streams queue progress as bounded deltas, batches Activity UI updates, and adds native startup plus packaged E2E release gates.
