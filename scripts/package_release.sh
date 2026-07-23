#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
python_cmd=python3
if ! command -v "$python_cmd" >/dev/null 2>&1; then
  python_cmd=python
fi

kind=${1:?kind is required}
target=${2:?target is required}
output_dir=${3:?output directory is required}
source=${4:?binary or bundle directory is required}
mkdir -p "$output_dir"
output_dir=$(cd "$output_dir" && pwd)
stage_root=$(mktemp -d)
name="video-compressor-${kind}-${target}"
stage="${stage_root}/${name}"
mkdir -p "$stage"
trap 'rm -rf "$stage_root"' EXIT

if [[ "$kind" == "cli" ]]; then
  test -f "$source"
  cp "$source" "$stage/$(basename "$source")"
  chmod +x "$stage/$(basename "$source")" 2>/dev/null || true
elif [[ "$kind" == "desktop" ]]; then
  test -d "$source"
  cp -R "$source" "$stage/bundle"
else
  echo "unsupported release kind: $kind" >&2
  exit 2
fi

cargo metadata --format-version 1 --locked > "$stage/SBOM.cargo-metadata.json"
{
  printf '%s\n\n' '# Third-party license manifest'
  printf 'Artifact kind: `%s`\nTarget: `%s`\n\n' "$kind" "$target"
  printf '%s\n\n' 'Generated from `cargo deny list`; the exact dependency graph is in SBOM.cargo-metadata.json.'
  cargo deny list
} > "$stage/THIRD_PARTY_LICENSES.md"
{
  printf 'target=%s\n' "$target"
  printf 'kind=%s\n' "$kind"
  printf 'source_revision=%s\n' "$(git rev-parse HEAD)"
  printf 'source_date_epoch=%s\n' "${SOURCE_DATE_EPOCH:-unset}"
  printf '%s\n' 'ffmpeg_bundle=none (thin artifact; provide matching external ffmpeg/ffprobe)'
} > "$stage/BUILD_INFO.txt"

if [[ "$target" == *-windows-* ]]; then
  archive="${output_dir}/${name}.zip"
  (cd "$stage_root" && zip -q -r "$archive" "$name")
else
  archive="${output_dir}/${name}.tar.gz"
  tar -czf "$archive" -C "$stage_root" "$name"
fi
"$python_cmd" "$script_dir/checksum.py" create "$archive"
printf 'created %s\n' "$archive"
