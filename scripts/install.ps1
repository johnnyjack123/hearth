# Installs hearth for the current user and starts `hearth setup`.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Zip <file or URL>] [-NoSetup] [-Target <folder>]
#
# Sources, first that applies:
#   -Zip            a release ZIP (file or URL)
#   this folder     the unpacked release ZIP, which contains this script
#   GitHub          the latest release, through the GitHub CLI (the
#                   repository is private, so a plain download needs a login)
#
# Target: %LOCALAPPDATA%\Programs\hearth, no admin rights. The VM service
# (hearth-vmd.exe) is copied below Program Files later by `hearth setup`,
# which asks for admin rights once.
# ASCII only: Windows PowerShell 5.1 reads scripts without BOM as ANSI.
param(
    [string]$Zip,
    [switch]$NoSetup,
    # Another folder, e.g. for a test that must not touch the real one.
    [string]$Target = (Join-Path $env:LOCALAPPDATA 'Programs\hearth')
)
$ErrorActionPreference = 'Stop'

$repository = 'johnnyjack123/hearth'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
$files = @('hearth.exe', 'hearthd.exe', 'hearth-vmd.exe')

function New-WorkDir {
    $dir = Join-Path ([IO.Path]::GetTempPath()) ('hearth-' + [Guid]::NewGuid())
    New-Item -ItemType Directory $dir | Out-Null
    return $dir
}

$source = $PSScriptRoot
if ($Zip) {
    $work = New-WorkDir
    $file = $Zip
    if ($Zip -match '^https?://') {
        $file = Join-Path $work 'hearth.zip'
        Invoke-WebRequest -Uri $Zip -OutFile $file -UseBasicParsing
    }
    Expand-Archive -Path $file -DestinationPath $work -Force
    $source = $work
} elseif (-not (Test-Path (Join-Path $source 'hearth.exe'))) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw "Hier liegt kein Release. -Zip <Datei oder URL> angeben oder die GitHub-CLI installieren (winget install GitHub.cli) und anmelden (gh auth login)."
    }
    $work = New-WorkDir
    gh release download --repo $repository --pattern "hearth-windows-$arch.zip" --dir $work
    if ($LASTEXITCODE -ne 0) { throw "gh release download ist gescheitert" }
    Expand-Archive -Path (Join-Path $work "hearth-windows-$arch.zip") -DestinationPath $work -Force
    $source = $work
}
foreach ($name in $files) {
    if (-not (Test-Path (Join-Path $source $name))) { throw "$name fehlt in $source" }
}

# Windows lets nobody open a running program for writing.
function Test-InUse([string] $Path) {
    if (-not (Test-Path $Path)) { return $false }
    try {
        [System.IO.File]::Open($Path, 'Open', 'Write', 'None').Close()
        return $false
    } catch {
        return $true
    }
}

# A running background service locks its files; setup starts it again. Its
# tray icon (hearthd) ends a moment after it, so wait for both.
$installed = Join-Path $Target 'hearth.exe'
if (Test-Path $installed) {
    & $installed daemon stop | Out-Null
    $service = @($installed, (Join-Path $Target 'hearthd.exe'))
    $deadline = (Get-Date).AddSeconds(30)
    while ((@($service | Where-Object { Test-InUse $_ }).Count -gt 0) -and ((Get-Date) -lt $deadline)) {
        Start-Sleep -Milliseconds 250
    }
}
New-Item -ItemType Directory -Force $Target | Out-Null
# Programs moved aside by an earlier update, as far as they ended.
Get-ChildItem $Target -Filter '*.exe.old*' -ErrorAction SilentlyContinue |
    Remove-Item -ErrorAction SilentlyContinue
foreach ($name in $files) {
    $copy = Join-Path $Target $name
    # Still running (hearth run in a terminal): a running program cannot be
    # replaced but can be renamed; it keeps running from there.
    if (Test-InUse $copy) {
        $n = 1
        while (Test-Path "$copy.old$n") { $n++ }
        Move-Item $copy "$copy.old$n"
    }
    Copy-Item (Join-Path $source $name) $copy -Force
    # Downloaded files carry the mark of the web; Windows would warn on every start.
    Unblock-File -Path $copy
}
Write-Output "hearth installiert in $Target"

if (-not $NoSetup) {
    & $installed setup
}
