#!/usr/bin/env bash
set -eu

if printf '%s\n' "$*" | grep -q -- '-version'; then
  echo 'ffmpeg version fake-1.0'
  exit 0
fi
if printf '%s\n' "$*" | grep -q -- '-encoders'; then
  cat <<'EOF'
 V..... libx265        libx265 fake encoder
 V..... libsvtav1      SVT-AV1 fake encoder
EOF
  exit 0
fi
if printf '%s\n' "$*" | grep -q -- '-hwaccels'; then
  printf 'Hardware acceleration methods:\n'
  printf ' videotoolbox\n'
  exit 0
fi
if printf '%s\n' "$*" | grep -q -- '-h encoder='; then
  printf '    -preset <value> (from 0 to 10)\n'
  printf '       slow\n       medium\n'
  exit 0
fi
if printf '%s\n' "$*" | grep -q -- '-c:v' && printf '%s\n' "$*" | grep -q -- '-f null'; then
  exit 0
fi

if printf '%s\n' "$*" | grep -q -- 'pipe:1'; then
  printf 'frame=1\nout_time_us=500000\nspeed=1x\nprogress=continue\n'
  printf 'frame=2\nout_time_us=1000000\nspeed=1x\nprogress=end\n'
fi

last=''
for value in "$@"; do last="$value"; done
if [ -n "$last" ] && [ "$last" != '-' ] && [ "$last" != 'pipe:1' ] && [ "$last" != '/dev/null' ] && [ "$last" != 'NUL' ]; then
  mkdir -p "$(dirname "$last")" 2>/dev/null || true
  : > "$last"
fi
exit 0
