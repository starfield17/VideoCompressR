# Architecture

VideoCompressR is a Rust modular monolith with two shared crates:

```text
vc-core <- vc-runtime <- video-compressor CLI
                      <- video-compressor-desktop Tauri adapter
```

`vc-core` contains serializable models, validation, bitrate and encoder
selection rules, planning, preview calculations, and the queue reducer. It has
no process, filesystem, Tokio, Tauri, Clap, or UI dependency.

`vc-runtime` owns tool discovery, FFprobe, capability detection and caching,
typed FFmpeg command rendering, direct process execution, progress parsing,
configuration/presets, scanning, subtitles, preview, activity events, and
`QueueSupervisor`.

The CLI parses the legacy command contract and formats output. Tauri commands
map generated DTOs to runtime services. React uses the typed API module only;
Rust queue snapshots are the single business-state source of truth.

The thin profile does not bundle FFmpeg. Release staging accepts a complete,
provenance-checked FFmpeg/FFprobe pair only.
