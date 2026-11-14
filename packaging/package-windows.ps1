# Packs a Windows release of Zakuro:
#   zakuro-setup-v<version>.exe       the installer, with zakuro.exe inside; it
#                                     downloads Zig for recompiling while it installs
#   zakuro-v<version>-windows-x64.zip zakuro.exe alone, for those who don't install
#
#   .\packaging\package-windows.ps1                 # build, then pack into release\
#   .\packaging\package-windows.ps1 -NoBuild        # pack the programs already built
#   .\packaging\package-windows.ps1 -ToolsOnly      # only put Zig next to target\release\zakuro.exe
param(
    [switch]$NoBuild,
    [switch]$ToolsOnly,
    [string]$OutDir = ""
)
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if ($OutDir -eq "") { $OutDir = Join-Path (Split-Path -Parent $Root) "release" }

$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Exe = Join-Path $Root "target\release\zakuro.exe"
$Setup = Join-Path $Root "target\release\zakuro-setup.exe"

function Invoke-Cargo([string[]]$Arguments) {
    # cargo reports progress on stderr, which Windows PowerShell would take
    # for a failure under Stop, so only its exit code decides
    $ErrorActionPreference = "Continue"
    cargo @Arguments
    $ErrorActionPreference = "Stop"
    if ($LASTEXITCODE -ne 0) { throw "cargo $($Arguments -join ' ') failed" }
}

if ($ToolsOnly) {
    # for testing recompiling with a local build: the Zig release
    # crates\zakuro\src\zig.rs looks for, as ziglang.org gives it
    $ZigVersion = "0.17.0"
    $ZigArchive = "zig-x86_64-windows-$ZigVersion.zip"
    $ZigSize = 100268137
    $ZigSha256 = "b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428"
    $Cache = Join-Path $Root "target\zig-download"
    New-Item -ItemType Directory -Force $Cache | Out-Null
    $Zip = Join-Path $Cache $ZigArchive
    function Test-Zig {
        (Test-Path $Zip) -and ((Get-Item $Zip).Length -eq $ZigSize) -and ((Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower() -eq $ZigSha256)
    }
    if (-not (Test-Zig)) {
        Write-Host "Downloading $ZigArchive"
        $ProgressPreference = "SilentlyContinue"
        Invoke-WebRequest "https://ziglang.org/download/$ZigVersion/$ZigArchive" -OutFile $Zip -UseBasicParsing
        if (-not (Test-Zig)) { Remove-Item $Zip -ErrorAction SilentlyContinue; throw "$ZigArchive does not match its known size and SHA-256" }
    }
    $Target = Join-Path (Split-Path $Exe) "tools\zig-$ZigVersion"
    if (-not (Test-Path (Join-Path $Target "zig.exe"))) {
        $Staging = Join-Path $Cache "unpacking"
        Remove-Item -Recurse -Force $Staging -ErrorAction SilentlyContinue
        Expand-Archive $Zip -DestinationPath $Staging
        $Inner = Get-ChildItem $Staging -Directory | Select-Object -First 1
        New-Item -ItemType Directory -Force (Split-Path $Target) | Out-Null
        Move-Item $Inner.FullName $Target
        Remove-Item -Recurse -Force $Staging
    }
    Write-Host "Zig $ZigVersion is in $(Split-Path $Target)"
    return
}

if (-not $NoBuild) {
    $Manifest = Join-Path $Root "Cargo.toml"
    Invoke-Cargo @("build", "--release", "--locked", "-p", "zakuro", "--manifest-path", $Manifest)
    # setup carries the zakuro.exe just built
    $env:ZAKURO_EXE = $Exe
    Invoke-Cargo @("build", "--release", "--locked", "-p", "zakuro-setup", "--manifest-path", $Manifest)
    Remove-Item Env:ZAKURO_EXE
}
if ((Get-Item $Setup).Length -lt (Get-Item $Exe).Length) { throw "zakuro-setup.exe doesn't carry zakuro.exe; build it with ZAKURO_EXE set" }

New-Item -ItemType Directory -Force $OutDir | Out-Null
$SetupOut = Join-Path $OutDir "zakuro-setup-v$Version.exe"
Copy-Item $Setup $SetupOut -Force

$Name = "zakuro-v$Version-windows-x64"
$Stage = Join-Path $OutDir $Name
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Stage | Out-Null
Copy-Item $Exe, (Join-Path $Root "LICENSE"), (Join-Path $Root "README.md") $Stage
Copy-Item (Join-Path $Root "crates\zakuro\fonts\Inter-LICENSE.txt") $Stage
$Out = Join-Path $OutDir "$Name.zip"
Remove-Item $Out -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Out -CompressionLevel Optimal

Write-Host ("Packed {0} ({1:N0} MB)" -f $SetupOut, ((Get-Item $SetupOut).Length / 1MB))
Write-Host ("Packed {0} ({1:N0} MB)" -f $Out, ((Get-Item $Out).Length / 1MB))
