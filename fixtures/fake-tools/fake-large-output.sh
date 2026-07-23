#!/usr/bin/env bash
set -eu

for index in $(seq 1 10000); do
  printf 'stdout-%s\n' "$index"
  printf 'stderr-%s\n' "$index" >&2
done
