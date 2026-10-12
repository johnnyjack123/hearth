# Watches a Windows folder with FileSystemWatcher (ReadDirectoryChangesW) for N seconds
# and prints every event. Used to check whether writes made inside a container
# (through the 9p/drvfs bind mount) are visible to Windows file watchers (IDEs).
# ASCII only.
param([string]$Path, [int]$Seconds = 10)

$w = New-Object System.IO.FileSystemWatcher $Path
$w.IncludeSubdirectories = $true
$w.EnableRaisingEvents = $true
$end = (Get-Date).AddSeconds($Seconds)
Write-Output "ready $Path"
while ((Get-Date) -lt $end) {
    $r = $w.WaitForChanged([System.IO.WatcherChangeTypes]::All, 500)
    if (-not $r.TimedOut) { Write-Output ("{0} {1} {2}" -f (Get-Date -Format HH:mm:ss.fff), $r.ChangeType, $r.Name) }
}
