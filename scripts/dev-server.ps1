#!/usr/bin/env pwsh
# Start the wanlogger backend server (development mode).
# Usage: pwsh scripts/dev-server.ps1 [-- extra args passed to `serve`]
#   -Port <int>     Listen port (default 9000)
#   -NoAuth         Disable auth (loopback only)
#   -Release        Build in release mode
[CmdletBinding()]
param(
    [int]    $Port    = 9000,
    [switch] $NoAuth,
    [switch] $Release
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$extraArgs = @("--port", $Port)
if ($NoAuth) { $extraArgs += "--no-auth" }

$buildFlag = if ($Release) { @("--release") } else { @() }
$cargoArgs  = @("run") + $buildFlag + @("-p", "wanlogger-cli", "--") + @("serve") + $extraArgs

Write-Host "Starting server: cargo $($cargoArgs -join ' ')" -ForegroundColor Cyan
& cargo @cargoArgs
