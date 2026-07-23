#!/usr/bin/env bash
set -eu

if [[ "${@: -1}" == *broken.mp4 ]]; then
  printf '%s\n' 'ffprobe fixture failure for broken.mp4' >&2
  exit 17
fi

cat <<'EOF'
{
  "streams": [
    {"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"30/1","bit_rate":"2000000"},
    {"codec_type":"audio","codec_name":"aac","bit_rate":"128000"}
  ],
  "format": {"duration":"2.0","bit_rate":"2128000"}
}
EOF
