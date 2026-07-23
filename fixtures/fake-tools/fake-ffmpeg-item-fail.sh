#!/usr/bin/env bash
set -eu

joined="$*"
if [[ "$joined" == *-version* ]]; then
  echo 'ffmpeg version fake-1.0'
  exit 0
fi
if [[ "$joined" == *-encoders* ]]; then
  cat <<'EOF'
 V..... libx265        libx265 fake encoder
 V..... hevc_qsv       hevc_qsv fake encoder
EOF
  exit 0
fi
if [[ "$joined" == *-hwaccels* ]]; then
  printf 'Hardware acceleration methods:\n'
  exit 0
fi
if [[ "$joined" == *'-h encoder='* ]]; then
  printf '       slow\n       medium\n'
  exit 0
fi
if [[ "$joined" == *fail-item* && "$joined" == *pipe:1* ]]; then
  printf 'item failure\n' >&2
  exit 17
fi
if [[ "$joined" == *pipe:1* ]]; then
  printf 'frame=1\nout_time_us=500000\nspeed=1x\nprogress=continue\n'
  printf 'frame=2\nout_time_us=1000000\nspeed=1x\nprogress=end\n'
fi
if [[ "$joined" == *missing-output-item* ]]; then
  exit 0
fi

last=''
for value in "$@"; do last="$value"; done
if [ -n "$last" ] && [ "$last" != '-' ] && [ "$last" != 'pipe:1' ] && [ "$last" != '/dev/null' ] && [ "$last" != 'NUL' ]; then
  mkdir -p "$(dirname "$last")" 2>/dev/null || true
  : > "$last"
fi
exit 0
