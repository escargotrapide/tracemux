#!/usr/bin/env pwsh
# Prepare the Tauri dev environment:
#   1. Build tracemux-cli (debug)
#   2. Copy binary to app-tauri/src-tauri/binaries/ (Tauri sidecar)
#   3. Generate a placeholder icon.png and icon.ico (if not already present)
#
# Run this once before `just dev-tauri` or `just dev-all`.
# In CI / release builds the real binary and proper icons are supplied externally.
[CmdletBinding()]
param(
    [switch] $Release
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot ? (Split-Path $PSScriptRoot) : $PWD

# 1. Build tracemux-cli
$buildFlag = if ($Release) { @("--release") } else { @() }
Write-Host "Building tracemux-cli..." -ForegroundColor Cyan
& cargo build @buildFlag -p tracemux-cli
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

# 2. Copy sidecar binary
$cargoProfile = if ($Release) { "release" } else { "debug" }
$srcBin    = Join-Path $root "target\$cargoProfile\tracemux.exe"
$binDir    = Join-Path $root "app-tauri\src-tauri\binaries"
$targetBin = Join-Path $binDir "tracemux-x86_64-pc-windows-msvc.exe"
New-Item -ItemType Directory -Path $binDir -Force | Out-Null
Copy-Item $srcBin $targetBin -Force
Write-Host "  Sidecar: $targetBin" -ForegroundColor DarkGreen

# 3. Generate placeholder icons (only if missing)
$iconDir = Join-Path $root "app-tauri\src-tauri\icons"
New-Item -ItemType Directory -Path $iconDir -Force | Out-Null

$iconPng = Join-Path $iconDir "icon.png"
if (-not (Test-Path $iconPng)) {
    # Minimal valid 1x1 RGB PNG
    $png = [byte[]]@(
        0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A,
        0x00,0x00,0x00,0x0D,0x49,0x48,0x44,0x52,
        0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x01,
        0x08,0x02,0x00,0x00,0x00,0x90,0x77,0x53,0xDE,
        0x00,0x00,0x00,0x0C,0x49,0x44,0x41,0x54,
        0x08,0xD7,0x63,0xF8,0xCF,0xC0,0x00,0x00,0x00,0x02,0x00,0x01,
        0xE2,0x21,0xBC,0x33,
        0x00,0x00,0x00,0x00,0x49,0x45,0x4E,0x44,0xAE,0x42,0x60,0x82)
    [System.IO.File]::WriteAllBytes($iconPng, $png)
    Write-Host "  icon.png (placeholder): $iconPng" -ForegroundColor DarkGreen
}

$iconIco = Join-Path $iconDir "icon.ico"
if (-not (Test-Path $iconIco)) {
    # Minimal valid 1x1 24bpp ICO
    $ico = [byte[]]@(
        0x00,0x00,0x01,0x00,0x01,0x00,
        0x01,0x01,0x00,0x00,0x01,0x00,0x18,0x00,
        0x30,0x00,0x00,0x00,0x16,0x00,0x00,0x00,
        0x28,0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x02,0x00,0x00,0x00,
        0x01,0x00,0x18,0x00,0x00,0x00,0x00,0x00,0x08,0x00,0x00,0x00,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00)
    [System.IO.File]::WriteAllBytes($iconIco, $ico)
    Write-Host "  icon.ico (placeholder): $iconIco" -ForegroundColor DarkGreen
}

Write-Host "dev-prepare complete. You can now run: just dev-tauri" -ForegroundColor Green
