# Publishes a Zakuro release from the stable branch:
#   checks, version bump, lint and tests, package, commit, push, GitHub release.
# Write ..\release\notes-v<version>.md first (no AI mentions).
#
#   .\packaging\release.ps1 -Version 0.3.4            # publish
#   .\packaging\release.ps1 -Version 0.3.4 -DryRun    # everything but commit, push and publish
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Workspace = Split-Path -Parent $Root
$ReleaseDir = Join-Path $Workspace "release"
$Notes = Join-Path $ReleaseDir "notes-v$Version.md"
$Manifest = Join-Path $Root "Cargo.toml"
$Gh = "C:\Program Files\GitHub CLI\gh.exe"
$Repo = "TVKain/zakuro"

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Run([string]$exe, [string[]]$arguments) {
    # native tools report progress on stderr; only the exit code decides
    $ErrorActionPreference = "Continue"
    & $exe @arguments
    $ErrorActionPreference = "Stop"
    if ($LASTEXITCODE -ne 0) { throw "$exe $($arguments -join ' ') failed" }
}
function Invoke-Git([string[]]$arguments) { Run "git.exe" (@("-C", $Root) + $arguments) }

Step "Checks"
if (-not (Test-Path $Notes)) { throw "write $Notes first" }
if (Select-String -Path $Notes -Pattern '\bAI\b|Claude|LLM|Co-authored' -CaseSensitive) { throw "$Notes mentions AI or co-authors" }
if ((git.exe -C $Root branch --show-current) -ne "stable") { throw "not on stable" }
if (git.exe -C $Root status --porcelain) { throw "uncommitted changes in $Root" }
if (git.exe -C $Root tag --list "v$Version") { throw "tag v$Version exists" }
if (-not $DryRun) {
    & $Gh release view "v$Version" --repo $Repo *> $null
    if ($LASTEXITCODE -eq 0) { throw "release v$Version exists on GitHub" }
}
$busy = Get-Process zakuro, zakuro-setup -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Root\target\release\*" }
if ($busy) { throw "close the zakuro built in target\release first (pid $($busy.Id -join ', '))" }

Step "Version $Version"
$text = [IO.File]::ReadAllText($Manifest)
$text = [regex]::Replace($text, '(?m)(\[workspace\.package\]\r?\nversion = ")[^"]+(")', "`${1}$Version`${2}")
[IO.File]::WriteAllText($Manifest, $text, (New-Object Text.UTF8Encoding $false))
Run "cargo" @("update", "-w", "--manifest-path", $Manifest)

try {
    Step "Lint and tests"
    Run "cargo" @("clippy", "--workspace", "--tests", "--manifest-path", $Manifest, "--", "-D", "warnings")
    Run "powershell" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Workspace "tools\test_regressions.ps1"), "-Quick")

    Step "Package"
    $out = if ($DryRun) { Join-Path $Root "target\release-dry" } else { $ReleaseDir }
    & (Join-Path $PSScriptRoot "package-windows.ps1") -OutDir $out
    $Setup = Join-Path $out "zakuro-setup-v$Version.exe"
    $Zip = Join-Path $out "zakuro-v$Version-windows-x64.zip"
} catch {
    Invoke-Git @("checkout", "--", "Cargo.toml", "Cargo.lock"); throw
}

if ($DryRun) {
    Invoke-Git @("checkout", "--", "Cargo.toml", "Cargo.lock")
    Step "Dry run done: $out (version bump undone)"
    return
}

Step "Commit and push"
Invoke-Git @("add", "Cargo.toml", "Cargo.lock")
Invoke-Git @("-c", "user.name=TVKain", "-c", "user.email=tvkain.it@gmail.com", "commit", "-q", "-m", "Release v$Version")
Invoke-Git @("push", "-q", "origin", "stable")

Step "GitHub release"
Run $Gh @("release", "create", "v$Version", $Setup, $Zip, "--repo", $Repo, "--target", "stable", "--title", "Zakuro v$Version", "--notes-file", $Notes)
Invoke-Git @("fetch", "-q", "--tags", "origin")

# release\ keeps only the latest; older ones are on GitHub
Get-ChildItem $ReleaseDir | Where-Object { $_.Name -notlike "*v$Version*" } | Remove-Item -Recurse -Force
Step "Released https://github.com/$Repo/releases/tag/v$Version"
