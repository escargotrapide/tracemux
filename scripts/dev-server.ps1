#!/usr/bin/env pwsh
# Start the tracemux backend server (development mode).
# Usage: pwsh scripts/dev-server.ps1 [options]
#   -Bind <host:port>   Bind address (default 127.0.0.1:9000)
#   -RequireAuth        Do not pass --no-auth (default: loopback --no-auth)
#   -NoAuth             Accepted for compatibility; loopback --no-auth is the default
#   -Release            Build in release mode
[CmdletBinding()]
param(
    [string] $Bind    = "127.0.0.1:9000",
    [switch] $NoAuth,
    [switch] $RequireAuth,
    [switch] $Release
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$extraArgs = @("--bind", $Bind)
if ($NoAuth -or -not $RequireAuth) { $extraArgs += "--no-auth" }

$buildFlag = if ($Release) { @("--release") } else { @() }
$cargoArgs  = @("run") + $buildFlag + @("-p", "tracemux-cli", "--") + @("serve") + $extraArgs

Write-Host "Starting server: cargo $($cargoArgs -join ' ')" -ForegroundColor Cyan
& cargo @cargoArgs
