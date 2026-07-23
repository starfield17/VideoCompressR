#!/usr/bin/env bash
set -eu

printf 'stdout-before\xff\nstdout-after\n'
printf 'stderr-before\xfe\nstderr-after\n' >&2
