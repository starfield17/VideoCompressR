#!/usr/bin/env bash
set -eu
trap 'exit 0' TERM INT
while true; do printf 'out_time_us=100000\nprogress=continue\n'; sleep 1; done
