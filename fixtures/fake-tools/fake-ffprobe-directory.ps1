$inputPath = if ($args.Count -gt 0) { $args[$args.Count - 1] } else { '' }
if ($inputPath -like '*broken.mp4') {
    [Console]::Error.WriteLine('ffprobe fixture failure for broken.mp4')
    exit 17
}
@'
{
  "streams": [
    {"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"30/1","bit_rate":"2000000"},
    {"codec_type":"audio","codec_name":"aac","bit_rate":"128000"}
  ],
  "format": {"duration":"2.0","bit_rate":"2128000"}
}
'@
exit 0
