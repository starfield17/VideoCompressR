#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
python_cmd=python3
if ! command -v "$python_cmd" >/dev/null 2>&1; then
  python_cmd=python
fi

directory=${1:?release directory is required}
shopt -s nullglob
archives=("$directory"/*.tar.gz "$directory"/*.zip)
test "${#archives[@]}" -gt 0
for archive in "${archives[@]}"; do
  checksum="${archive}.sha256"
  test -f "$checksum"
  "$python_cmd" "$script_dir/checksum.py" verify "$checksum"
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
