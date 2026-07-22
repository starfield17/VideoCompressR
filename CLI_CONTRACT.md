# CLI contract

Binary name: `video-compressor`.

Commands retained from the reference implementation:

```text
plan INPUT [runtime and encode flags]
encode INPUT [runtime and encode flags]
preview INPUT [runtime, encode, and sample flags]
preset list|load NAME|save NAME|delete NAME
```

Important flags include `--codec`, `--backend`, `--decode-acceleration`,
`--parallel`, `--parallel-backends`, `--ratio`, `--min-video-kbps`,
`--max-video-kbps`, `--container`, `--audio-mode`, `--audio-bitrate`,
`--copy-subtitles`, `--copy-external-subtitles`, `--two-pass`, `--overwrite`,
`--encoder-preset`, `--pix-fmt`, `--maxrate-factor`, `--bufsize-factor`,
`--dry-run`, `--output`, `--workdir`, `--ffmpeg`, `--ffprobe`, `--preset`,
`--recursive`, and `--lang`.

Exit codes are stable: `0` success, `2` usage or configuration/serialization
failure, `3` tool discovery/capability failure, `4` planning/probe failure,
`5` encode/preview failure, and `130` cancellation. Diagnostics go to stderr;
successful plan/result data goes to stdout.
