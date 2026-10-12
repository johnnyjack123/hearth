# Sums the memory of an app process and all of its descendants (e.g. a Tauri app and
# its msedgewebview2.exe children). Run it while the app is open, then again after
# minimizing / hiding the window.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File measure-app-ram.ps1 -Name tauri-ram -Label "window open"
# ASCII only (Windows PowerShell 5.1 reads BOM-less files as ANSI).
param(
    [string]$Name = "tauri-ram",
    [string]$Label = "sample",
    [int]$Samples = 5,
    [int]$IntervalSec = 2
)

function Get-Tree([int]$RootId, $All) {
    $ids = @($RootId)
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($RootId)
    while ($queue.Count -gt 0) {
        $p = $queue.Dequeue()
        foreach ($c in ($All | Where-Object { $_.ParentProcessId -eq $p })) {
            $ids += [int]$c.ProcessId
            $queue.Enqueue([int]$c.ProcessId)
        }
    }
    return $ids
}

$root = Get-Process -Name $Name -ErrorAction SilentlyContinue | Sort-Object StartTime | Select-Object -First 1
if (-not $root) { Write-Output "process '$Name' not running"; exit 1 }

$ws = @(); $priv = @(); $count = 0
for ($i = 0; $i -lt $Samples; $i++) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, WorkingSetSize, PrivatePageCount
    $ids = Get-Tree $root.Id $all
    $procs = $all | Where-Object { $ids -contains [int]$_.ProcessId }
    $count = @($procs).Count
    $ws += (($procs | Measure-Object -Property WorkingSetSize -Sum).Sum / 1MB)
    $priv += (($procs | Measure-Object -Property PrivatePageCount -Sum).Sum / 1MB)
    if ($i -lt $Samples - 1) { Start-Sleep -Seconds $IntervalSec }
}

$procs | Sort-Object Name | ForEach-Object { "  {0,-28} pid {1,6}  ws {2,7:N1} MB  private {3,7:N1} MB" -f $_.Name, $_.ProcessId, ($_.WorkingSetSize / 1MB), ($_.PrivatePageCount / 1MB) }
"{0}: {1} processes, working set avg {2:N0} MB, private avg {3:N0} MB ({4} samples)" -f $Label, $count, ($ws | Measure-Object -Average).Average, ($priv | Measure-Object -Average).Average, $Samples
