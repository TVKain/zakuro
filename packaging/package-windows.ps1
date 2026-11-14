# Packs a Windows release of Zakuro: zakuro.exe with the Zig compiler that
# recompiling games uses, in tools\zig-<version> next to it, so that nothing
# has to be downloaded or installed after unzipping.
#
#   .\packaging\package-windows.ps1                 # build, then pack into release\
#   .\packaging\package-windows.ps1 -NoBuild        # pack the exe already built
#   .\packaging\package-windows.ps1 -ToolsOnly      # only put Zig next to target\release\zakuro.exe
param(
    [switch]$NoBuild,
    [switch]$ToolsOnly,
    [string]$OutDir = ""
)
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if ($OutDir -eq "") { $OutDir = Join-Path (Split-Path -Parent $Root) "release" }

# the Zig release crates\zakuro\src\zig.rs looks for, and its archive as
# ziglang.org/download/index.json gives it
$ZigVersion = "0.17.0"
$ZigArchive = "zig-x86_64-windows-$ZigVersion.zip"
$ZigSize = 100268137
$ZigSha256 = "b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428"

$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Exe = Join-Path $Root "target\release\zakuro.exe"

if (-not $NoBuild -and -not $ToolsOnly) {
    cargo build --release --locked -p zakuro --manifest-path (Join-Path $Root "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

# the archive, downloaded once into target\ and checked every time
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

# unpacked once into target\, its single folder renamed to zig-<version>
$Unpacked = Join-Path $Cache "zig-$ZigVersion"
if (-not (Test-Path (Join-Path $Unpacked "zig.exe"))) {
    $Staging = Join-Path $Cache "unpacking"
    Remove-Item -Recurse -Force $Staging -ErrorAction SilentlyContinue
    Expand-Archive $Zip -DestinationPath $Staging
    $Inner = Get-ChildItem $Staging -Directory | Select-Object -First 1
    Remove-Item -Recurse -Force $Unpacked -ErrorAction SilentlyContinue
    Move-Item $Inner.FullName $Unpacked
    Remove-Item -Recurse -Force $Staging
}

function Copy-Zig([string]$Into) {
    $Target = Join-Path $Into "tools\zig-$ZigVersion"
    if (-not (Test-Path (Join-Path $Target "zig.exe"))) {
        New-Item -ItemType Directory -Force (Split-Path $Target) | Out-Null
        Copy-Item -Recurse $Unpacked $Target
    }
}

if ($ToolsOnly) {
    Copy-Zig (Split-Path $Exe)
    Write-Host "Zig $ZigVersion is in $(Join-Path (Split-Path $Exe) 'tools')"
    return
}

$Name = "zakuro-v$Version-windows-x64"
$Stage = Join-Path $OutDir $Name
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Stage | Out-Null
Copy-Item $Exe, (Join-Path $Root "LICENSE"), (Join-Path $Root "README.md") $Stage
Copy-Item (Join-Path $Root "crates\zakuro\fonts\Inter-LICENSE.txt") $Stage
Copy-Zig $Stage

$Out = Join-Path $OutDir "$Name.zip"
Remove-Item $Out -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Out -CompressionLevel Optimal
Write-Host ("Packed {0} ({1:N0} MB)" -f $Out, ((Get-Item $Out).Length / 1MB))
