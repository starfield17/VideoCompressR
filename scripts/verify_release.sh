#!/usr/bin/env bash
set -euo pipefail

directory=${1:?release directory is required}
shopt -s nullglob
archives=("$directory"/*.tar.gz "$directory"/*.zip)
test "${#archives[@]}" -gt 0
for archive in "${archives[@]}"; do
  checksum="${archive}.sha256"
  test -f "$checksum"
  (cd "$(dirname "$archive")" && sha256sum -c "$(basename "$checksum")")
  if [[ "$archive" == *.zip ]]; then
    names=$(unzip -Z1 "$archive")
  else
    names=$(tar -tzf "$archive")
  fi
  grep -q '/BUILD_INFO.txt$' <<<"$names"
  grep -q '/SBOM.cargo-metadata.json$' <<<"$names"
  grep -q '/THIRD_PARTY_LICENSES.md$' <<<"$names"
  printf 'verified %s\n' "$(basename "$archive")"
done
