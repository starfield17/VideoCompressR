$joined = $args -join ' '
if ($joined -match '-version') { Write-Output 'ffmpeg version fake-1.0'; exit 0 }
if ($joined -match '-encoders') { Write-Output ' V..... libx265 fake encoder'; Write-Output ' V..... libsvtav1 fake encoder'; exit 0 }
if ($joined -match '-hwaccels') { Write-Output 'Hardware acceleration methods:'; Write-Output ' videotoolbox'; exit 0 }
if ($joined -match '-h encoder=') { Write-Output '       slow'; Write-Output '       medium'; exit 0 }
if (($joined -match '-c:v') -and ($joined -match '-f null')) { exit 0 }
if ($joined -match 'pipe:1') { Write-Output 'frame=1'; Write-Output 'out_time_us=500000'; Write-Output 'speed=1x'; Write-Output 'progress=continue'; Write-Output 'frame=2'; Write-Output 'out_time_us=1000000'; Write-Output 'speed=1x'; Write-Output 'progress=end' }
$last = if ($args.Count -gt 0) { $args[$args.Count - 1] } else { '' }
if ($last -and $last -notin @('-', 'pipe:1', '/dev/null', 'NUL')) { $parent = Split-Path -Parent $last; if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }; New-Item -ItemType File -Force -Path $last | Out-Null }
exit 0
