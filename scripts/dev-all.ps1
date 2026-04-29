#!/usr/bin/env pwsh
# Start both the backend server and the web UI dev server together.
# Each process opens in a new PowerShell window (Windows) or background job.
# Usage: pwsh scripts/dev-all.ps1 [-Port <int>] [-NoAuth] [-Url <wss://...>]
#   -Port <int>    Backend listen port (default 9000)
#   -NoAuth        Disable auth (loopback only)
#   -Url <uri>     Override WS URL for the web UI (defaults to wss://localhost:<Port>/ws)
[CmdletBinding()]
param(
    [int]    $Port  = 9000,
    [switch] $NoAuth,
    [string] $Url   = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $Url) { $Url = "wss://localhost:$Port/ws" }

$root = $PSScriptRoot ? (Split-Path $PSScriptRoot) : $PWD

Write-Host "Launching backend server on port $Port ..." -ForegroundColor Cyan
$serverArgs = @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $root "scripts\dev-server.ps1"), "-Port", $Port)
if ($NoAuth) { $serverArgs += "-NoAuth" }

$serverProc = Start-Process pwsh -ArgumentList $serverArgs -PassThru

Write-Host "Waiting 3 s for backend to start ..." -ForegroundColor DarkYellow
Start-Sleep 3

Write-Host "Launching Web UI (backend: $Url) ..." -ForegroundColor Cyan
$env:VITE_WANLOGGER_URL = $Url
$webJob = Start-Job -ScriptBlock {
    param($r, $u)
    $env:VITE_WANLOGGER_URL = $u
    Set-Location $r
    & pnpm --filter ./web dev
} -ArgumentList $root, $Url

Write-Host ""
Write-Host "Both processes started." -ForegroundColor Green
Write-Host "  Backend PID : $($serverProc.Id)"
Write-Host "  Web UI Job  : $($webJob.Id)"
Write-Host "  Web UI URL  : http://localhost:5173"
Write-Host ""
Write-Host "Press Ctrl+C to stop all." -ForegroundColor DarkYellow

try {
    while ($true) {
        $out = Receive-Job -Job $webJob
        if ($out) { Write-Host $out }
        if ($serverProc.HasExited) {
            Write-Warning "Backend process exited (code $($serverProc.ExitCode))."
            break
        }
        Start-Sleep -Milliseconds 500
    }
} finally {
    Stop-Job -Job $webJob -ErrorAction SilentlyContinue
    Remove-Job -Job $webJob -Force -ErrorAction SilentlyContinue
    if (-not $serverProc.HasExited) { Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue }
    Write-Host "Stopped." -ForegroundColor DarkYellow
}
