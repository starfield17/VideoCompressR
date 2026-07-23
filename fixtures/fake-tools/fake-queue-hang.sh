#!/usr/bin/env bash
set -eu

joined="$*"
if [[ "$joined" == *-version* || "$joined" == *-encoders* || "$joined" == *-hwaccels* || "$joined" == *'-h encoder='* || "$joined" == *'-f lavfi'* ]]; then
  exec "$(dirname "$0")/fake-ffmpeg-item-fail.sh" "$@"
fi
trap 'exit 0' TERM INT
while true; do
  printf 'out_time_us=100000\nprogress=continue\n'
  sleep 1
done
