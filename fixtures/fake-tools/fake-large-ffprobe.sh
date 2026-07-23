#!/usr/bin/env bash
set -eu

printf '%s' '{"streams":[{"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"30/1","bit_rate":"2000000"}],"format":{"duration":"2.0","metadata":{'
for index in $(seq 1 10000); do
  if [ "$index" -gt 1 ]; then printf ','; fi
  printf '"key-%s":"value-%s"' "$index" "$index"
done
printf '%s\n' '}}}'
