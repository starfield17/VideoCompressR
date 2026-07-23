#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
python_cmd=python3
if ! command -v "$python_cmd" >/dev/null 2>&1; then
  python_cmd=python
fi

directory=${1:?release directory is required}
verify_tmp=$(mktemp -d)
trap 'rm -rf "$verify_tmp"' EXIT
shopt -s nullglob
archives=("$directory"/*.tar.gz "$directory"/*.zip)
test "${#archives[@]}" -gt 0
for archive in "${archives[@]}"; do
  checksum="${archive}.sha256"
  test -f "$checksum"
  "$python_cmd" "$script_dir/checksum.py" verify "$checksum"
  if [[ "$archive" == *.zip ]]; then
    names=$(unzip -Z1 "$archive")
    unzip -q "$archive" -d "$verify_tmp"
  else
    names=$(tar -tzf "$archive")
    tar -xzf "$archive" -C "$verify_tmp"
  fi
  grep -q '/BUILD_INFO.txt$' <<<"$names"
  grep -q '/SBOM.cargo-metadata.json$' <<<"$names"
  grep -q '/THIRD_PARTY_LICENSES.md$' <<<"$names"
  grep -q '/SIGNING_STATUS.txt$' <<<"$names"
  signing_status=$(find "$verify_tmp" -type f -name SIGNING_STATUS.txt -print -quit)
  test -n "$signing_status"
  grep -qx 'signing=unsigned' "$signing_status"
  rm -rf "$verify_tmp"/*
  printf 'verified %s\n' "$(basename "$archive")"
done
