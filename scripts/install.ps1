# Installs llm-harness for the current user and starts `harness setup`.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Zip <file or URL>] [-NoSetup] [-Target <folder>]
#
# Sources, first that applies:
#   -Zip            a release ZIP (file or URL)
#   this folder     the unpacked release ZIP, which contains this script
#   GitHub          the latest release, through the GitHub CLI (the
#                   repository is private, so a plain download needs a login)
#
# Target: %LOCALAPPDATA%\Programs\llm-harness, no admin rights. The VM service
# (harness-vmd.exe) is copied below Program Files later by `harness setup`,
# which asks for admin rights once.
# ASCII only: Windows PowerShell 5.1 reads scripts without BOM as ANSI.
param(
    [string]$Zip,
    [switch]$NoSetup,
    # Another folder, e.g. for a test that must not touch the real one.
    [string]$Target = (Join-Path $env:LOCALAPPDATA 'Programs\llm-harness')
)
$ErrorActionPreference = 'Stop'

$repository = 'johnnyjack123/llm-harness'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
$files = @('harness.exe', 'harnessd.exe', 'harness-vmd.exe')

function New-WorkDir {
    $dir = Join-Path ([IO.Path]::GetTempPath()) ('llm-harness-' + [Guid]::NewGuid())
    New-Item -ItemType Directory $dir | Out-Null
    return $dir
}

$source = $PSScriptRoot
if ($Zip) {
    $work = New-WorkDir
    $file = $Zip
    if ($Zip -match '^https?://') {
        $file = Join-Path $work 'llm-harness.zip'
        Invoke-WebRequest -Uri $Zip -OutFile $file -UseBasicParsing
    }
    Expand-Archive -Path $file -DestinationPath $work -Force
    $source = $work
} elseif (-not (Test-Path (Join-Path $source 'harness.exe'))) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw "Hier liegt kein Release. -Zip <Datei oder URL> angeben oder die GitHub-CLI installieren (winget install GitHub.cli) und anmelden (gh auth login)."
    }
    $work = New-WorkDir
    gh release download --repo $repository --pattern "llm-harness-windows-$arch.zip" --dir $work
    if ($LASTEXITCODE -ne 0) { throw "gh release download ist gescheitert" }
    Expand-Archive -Path (Join-Path $work "llm-harness-windows-$arch.zip") -DestinationPath $work -Force
    $source = $work
}
foreach ($name in $files) {
    if (-not (Test-Path (Join-Path $source $name))) { throw "$name fehlt in $source" }
}

# A running background service locks its files; setup starts it again.
$installed = Join-Path $Target 'harness.exe'
if (Test-Path $installed) {
    & $installed daemon stop | Out-Null
}
New-Item -ItemType Directory -Force $Target | Out-Null
foreach ($name in $files) {
    $copy = Join-Path $Target $name
    Copy-Item (Join-Path $source $name) $copy -Force
    # Downloaded files carry the mark of the web; Windows would warn on every start.
    Unblock-File -Path $copy
}
Write-Output "harness installiert in $Target"

if (-not $NoSetup) {
    & $installed setup
}
