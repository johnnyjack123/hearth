# Samples the memory of the WSL VM process (vmmemWSL / vmmem) on the Windows host.
# Usage: powershell -NoProfile -File measure-vmmem.ps1 -Label "idle" -Samples 10 -IntervalSec 3
# Output: one line "label;samples;ws_avg_mb;ws_min_mb;ws_max_mb;priv_avg_mb"
# ASCII only (Windows PowerShell 5.1 reads BOM-less files as ANSI).
param(
    [string]$Label = "sample",
    [int]$Samples = 10,
    [int]$IntervalSec = 3
)

$ws = @()
$priv = @()
for ($i = 0; $i -lt $Samples; $i++) {
    $procs = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^vmmem' }
    if ($procs) {
        $ws += (($procs | Measure-Object -Property WorkingSet64 -Sum).Sum / 1MB)
        $priv += (($procs | Measure-Object -Property PrivateMemorySize64 -Sum).Sum / 1MB)
    }
    if ($i -lt $Samples - 1) { Start-Sleep -Seconds $IntervalSec }
}

if ($ws.Count -eq 0) {
    Write-Output "$Label;0;n/a;n/a;n/a;n/a"
    exit 1
}

$wsStat = $ws | Measure-Object -Average -Minimum -Maximum
$privStat = $priv | Measure-Object -Average
"{0};{1};{2:N0};{3:N0};{4:N0};{5:N0}" -f $Label, $ws.Count, $wsStat.Average, $wsStat.Minimum, $wsStat.Maximum, $privStat.Average
