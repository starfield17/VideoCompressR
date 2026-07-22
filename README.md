# VideoCompressR

Rust + Tauri 2 rewrite of the legacy video compressor. The root build is
independent of `reffer/`; that directory is retained only as a read-only
reference fixture.

## Development

Install Rust 1.95.0, Node 22, pnpm 11, and an external matching `ffmpeg` /
`ffprobe` pair. Then run:

```bash
pnpm install --frozen-lockfile
cargo test --workspace --locked
pnpm typecheck
pnpm lint
pnpm test:run
pnpm build
```

The CLI is `video-compressor`; `cargo run --bin video-compressor -- --help`
shows the stable command surface. `pnpm tauri dev` starts the desktop app.

`VIDEO_COMPRESSOR_DATA_DIR` can point tests or a local run at an isolated data
directory. The desktop bundle is thin and does not ship FFmpeg.

See [ARCHITECTURE.md](ARCHITECTURE.md), [CLI_CONTRACT.md](CLI_CONTRACT.md),
[UX_CONTRACT.md](UX_CONTRACT.md), and [RELEASE_CONTRACT.md](RELEASE_CONTRACT.md)
for the frozen boundaries and compatibility contracts.
