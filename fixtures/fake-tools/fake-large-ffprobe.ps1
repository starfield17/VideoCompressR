[Console]::Out.Write('{"streams":[{"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"30/1","bit_rate":"2000000"}],"format":{"duration":"2.0","metadata":{')
for ($index = 1; $index -le 10000; $index++) {
    if ($index -gt 1) { [Console]::Out.Write(',') }
    [Console]::Out.Write(('"key-{0}":"value-{0}"' -f $index))
}
[Console]::Out.WriteLine('}}}')
exit 0
