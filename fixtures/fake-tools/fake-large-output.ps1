1..10000 | ForEach-Object {
    [Console]::Out.WriteLine("stdout-$_")
    [Console]::Error.WriteLine("stderr-$_")
}
exit 0
