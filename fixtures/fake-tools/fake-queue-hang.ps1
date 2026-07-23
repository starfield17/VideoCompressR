$joined = $args -join ' '
if ($joined -match '-version' -or $joined -match '-encoders' -or $joined -match '-hwaccels' -or $joined -match '-h encoder=' -or $joined -match '-f lavfi') { & "$PSScriptRoot/fake-ffmpeg-item-fail.ps1" @args; exit $LASTEXITCODE }
while ($true) { Write-Output 'out_time_us=100000'; Write-Output 'progress=continue'; Start-Sleep -Seconds 1 }
