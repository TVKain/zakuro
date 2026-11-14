# Zakuro's regression pipeline: lint, unit tests, integration suites, shader
# compilation and (unless -Quick) the release build with its Windows checks.
# See docs/testing.md.
#
#   .\tools\test_regressions.ps1           # everything (a few minutes)
#   .\tools\test_regressions.ps1 -Quick    # skips the release build (about a minute)
#
# glslangValidator is looked for in -Glslang, $env:GLSLANG_VALIDATOR,
# tools\glslang\bin, PATH and $env:VULKAN_SDK, in that order.

param(
    [switch]$Quick,          # skip the release build
    [switch]$VerboseOutput,  # print test output
    [string]$Glslang = ""
)

$ErrorActionPreference = "Continue"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$OnWindows = $env:OS -eq "Windows_NT"
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

function Find-Glslang {
    $exe = if ($OnWindows) { "glslangValidator.exe" } else { "glslangValidator" }
    $candidates = @(
        $Glslang,
        $env:GLSLANG_VALIDATOR,
        (Join-Path $Root "tools\glslang\bin\$exe")
    )
    $onPath = Get-Command glslangValidator -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += $onPath.Source }
    if ($env:VULKAN_SDK) {
        $candidates += (Join-Path $env:VULKAN_SDK "Bin\$exe")
        $candidates += (Join-Path $env:VULKAN_SDK "bin/$exe")
    }
    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) { return (Resolve-Path $c).Path }
    }
    return $null
}

Write-Host "========================================================================" -ForegroundColor Cyan
Write-Host "                 ZAKURO REGRESSION PIPELINE                             " -ForegroundColor Cyan
Write-Host "========================================================================" -ForegroundColor Cyan
Write-Host "Repository : $Root"
Write-Host "Mode       : $(if ($Quick) { 'Quick (no release build)' } else { 'Full' })"
Write-Host ""

$Results = [System.Collections.Generic.List[PSCustomObject]]::new()

function Show-Summary {
    $Stopwatch.Stop()
    $total = [Math]::Round($Stopwatch.Elapsed.TotalSeconds, 2)
    Write-Host "========================================================================" -ForegroundColor Cyan
    $Results | Format-Table -Property Step, Status, Duration, Details -AutoSize
    $failed = @($Results | Where-Object { $_.Status -ne "PASS" })
    if ($failed.Count -eq 0) {
        Write-Host "ALL CHECKS PASSED in $total s." -ForegroundColor Green
    } else {
        Write-Host "PIPELINE FAILED with $($failed.Count) error(s) in $total s." -ForegroundColor Red
    }
}

function Run-Step([string]$Name, [scriptblock]$Action) {
    Write-Host ">>> $Name..." -ForegroundColor Yellow
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        & $Action
        $elapsed = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
        Write-Host "  [PASS] $Name ($elapsed s)`n" -ForegroundColor Green
        $Results.Add([PSCustomObject]@{ Step = $Name; Status = "PASS"; Duration = "$elapsed s"; Details = "" })
    } catch {
        $elapsed = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
        Write-Host "  [FAIL] $Name ($elapsed s)" -ForegroundColor Red
        Write-Host "  $_`n" -ForegroundColor Red
        $Results.Add([PSCustomObject]@{ Step = $Name; Status = "FAIL"; Duration = "$elapsed s"; Details = ("$_" -split "`n")[0] })
        Show-Summary
        exit 1
    }
}

function Invoke-Cargo([string[]]$arguments) {
    Push-Location $Root
    try {
        $output = & cargo @arguments 2>&1
        if ($LASTEXITCODE -ne 0) { throw "cargo $($arguments -join ' ') failed (exit $LASTEXITCODE)`n$($output | Out-String)" }
        if ($VerboseOutput) { Write-Host ($output | Out-String) }
    } finally {
        Pop-Location
    }
}

# 1. lint: every crate, test, example and bench, warnings are errors
Run-Step "Step 1: Clippy (all targets, -D warnings)" {
    Invoke-Cargo @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
}

# 2. unit tests inside every crate
Run-Step "Step 2: Workspace unit tests" {
    Invoke-Cargo @("test", "--workspace", "--lib")
}

# 3. the frontend's integration suites, crates/zakuro/tests
Run-Step "Step 3: Integration regression suites" {
    Invoke-Cargo @("test", "-p", "zakuro", "--test", "*")
}

# 4. every shader still compiles. the committed .spv files are what the
# program embeds; this only checks the sources, into a temporary folder.
Run-Step "Step 4: Shaders compile" {
    $glslang = Find-Glslang
    if (-not $glslang) {
        throw "glslangValidator not found. Install the Vulkan SDK, put glslang in tools\glslang\bin, or pass -Glslang <path> (docs/getting-started.md)."
    }
    $shaders = Join-Path $Root "crates\zakuro-gpu\shaders"
    # output .spv => source and defines; must cover every .spv in the folder
    $builds = [ordered]@{
        "present.vert.spv"        = @("present.vert")
        "present.frag.spv"        = @("present.frag")
        "present_stereo.frag.spv" = @("present.frag", "-DSTEREO")
        "overlay.vert.spv"        = @("overlay.vert")
        "overlay.frag.spv"        = @("overlay.frag")
        "shade.vert.spv"          = @("shade.vert")
        "raster.vert.spv"         = @("raster.vert")
        "raster.frag.spv"         = @("raster.frag")
        "raster_depth.frag.spv"   = @("raster.frag", "-DWRITES_DEPTH")
        "transfer.comp.spv"       = @("transfer.comp")
        "depth.comp.spv"          = @("depth.comp")
        "upright.comp.spv"        = @("upright.comp")
        "upright_image.comp.spv"  = @("upright.comp", "-DTO_IMAGE")
        "scale.comp.spv"          = @("scale.comp")
    }
    $unlisted = Get-ChildItem $shaders -Filter *.spv | Where-Object { -not $builds.Contains($_.Name) }
    if ($unlisted) { throw "add these to the shader list in tools\test_regressions.ps1: $($unlisted.Name -join ', ')" }
    $temp = Join-Path ([IO.Path]::GetTempPath()) "zakuro-shader-check-$PID"
    New-Item -ItemType Directory $temp -Force | Out-Null
    try {
        foreach ($out in $builds.Keys) {
            $source = Join-Path $shaders $builds[$out][0]
            $defines = @($builds[$out] | Select-Object -Skip 1)
            $output = & $glslang -V @defines $source -o (Join-Path $temp $out) 2>&1
            if ($LASTEXITCODE -ne 0) { throw "$out did not compile:`n$($output | Out-String)" }
        }
    } finally {
        Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 5. the release program, and on Windows that it is a GUI program with an icon
if (-not $Quick) {
    Run-Step "Step 5: Release build" {
        Invoke-Cargo @("build", "--release", "-p", "zakuro")
        if (-not $OnWindows) { return }
        $exe = Join-Path $Root "target\release\zakuro.exe"
        if (-not (Test-Path $exe)) { throw "release binary not found at $exe" }
        # PE optional header: Subsystem 2 is IMAGE_SUBSYSTEM_WINDOWS_GUI, anything
        # else opens a console window next to Zakuro
        $stream = [System.IO.File]::OpenRead($exe)
        $reader = New-Object System.IO.BinaryReader($stream)
        try {
            $stream.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
            $pe = $reader.ReadInt32()
            $stream.Seek($pe + 24 + 68, [System.IO.SeekOrigin]::Begin) | Out-Null
            $subsystem = $reader.ReadUInt16()
        } finally {
            $reader.Close()
        }
        if ($subsystem -ne 2) { throw "PE subsystem is $subsystem, expected 2 (Windows GUI)" }
        Add-Type -AssemblyName System.Drawing
        if ($null -eq [System.Drawing.Icon]::ExtractAssociatedIcon($exe)) { throw "no icon embedded in $exe" }
    }
}

Show-Summary
exit 0
