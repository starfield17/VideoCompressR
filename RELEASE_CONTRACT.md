# Release contract

Thin artifacts are built natively for:

```text
windows-x86_64  windows-arm64
macos-x86_64    macos-arm64
linux-x86_64    linux-arm64
```

Each target publishes a CLI archive and a Tauri desktop package where the
runner supports it. Archives include checksums, a Cargo metadata SBOM,
`cargo-deny` license output, and a package smoke check. Signing is conditional
on a future platform-specific signing implementation; the current publish
workflow always labels artifacts `signing=unsigned` and does not infer signing
from secret presence. Full FFmpeg bundles require a matching ffmpeg/ffprobe
pair, target identity, hash, source revision, build flags, and license
provenance.
