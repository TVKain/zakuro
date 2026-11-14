# Runs a game in a sandbox with a replay and a ZAKURO_TIMING record, then
# reports input lag, frame pacing and audio sync with timing_report.py.
# See docs/timing-and-audio.md.
#
#   .\tools\timing_run.ps1 -Rom <game.3ds> -Sandbox <scratch folder> -Out <run.txt>
#
# The sandbox is used as APPDATA: settings, saves and recompiled code come from
# <Sandbox>\zakuro and <Sandbox>\3dsrecomp, never from the real profile. Edit
# <Sandbox>\zakuro\settings.toml for the frame rate limit, resolution and so on.
param(
    [Parameter(Mandatory)][string]$Rom,
    [Parameter(Mandatory)][string]$Sandbox,
    [Parameter(Mandatory)][string]$Out,
    [string]$Replay = (Join-Path $PSScriptRoot "replays\ultra-sun-overworld-walk.txt"),
    [int]$Seconds = 140,
    [string]$Exe = (Join-Path $PSScriptRoot "..\target\release\zakuro.exe"),
    [int]$FirstFrame = 5600   # where the replay's walking starts
)
$ErrorActionPreference = "Stop"
$Exe = (Resolve-Path $Exe).Path
$Sandbox = (New-Item -ItemType Directory -Force $Sandbox).FullName
if ([IO.Path]::GetFullPath($Sandbox).TrimEnd('\') -ieq [Environment]::GetFolderPath("ApplicationData").TrimEnd('\')) {
    throw "the sandbox is the real APPDATA; pick a scratch folder"
}

$realAppData = $env:APPDATA
$env:APPDATA = $Sandbox
$env:ZAKURO_REPLAY = (Resolve-Path $Replay).Path
$env:ZAKURO_TIMING = [IO.Path]::GetFullPath($Out)
try {
    $process = Start-Process $Exe -ArgumentList "`"$Rom`"" -PassThru
    Start-Sleep -Seconds $Seconds
    if (-not $process.HasExited) {
        $process.CloseMainWindow() | Out-Null
        if (-not $process.WaitForExit(5000)) { $process | Stop-Process -Force }
    }
} finally {
    # this shell keeps its own profile afterwards
    $env:APPDATA = $realAppData
    Remove-Item Env:ZAKURO_REPLAY, Env:ZAKURO_TIMING -ErrorAction SilentlyContinue
}

$python = (Get-Command python, py -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if ($python) {
    & $python (Join-Path $PSScriptRoot "timing_report.py") $Out $FirstFrame
} else {
    Write-Host "Python not found; run tools\timing_report.py $Out $FirstFrame yourself."
}
