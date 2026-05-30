#!/usr/bin/env pwsh
# Start both the backend server and the web UI dev server together.
# Each process opens in a new PowerShell window (Windows) or background job.
# Usage: pwsh scripts/dev-all.ps1 [-Bind <host:port>] [-RequireAuth] [-Url <ws://...>]
#   -Bind <h:p>    Backend bind address (default 127.0.0.1:9000)
#   -RequireAuth   Do not pass --no-auth (default: loopback --no-auth)
#   -NoAuth        Accepted for compatibility; loopback --no-auth is the default
#   -Url <uri>     Override WS URL for the web UI (defaults to ws://127.0.0.1:<port>/ws)
[CmdletBinding()]
param(
    [string] $Bind  = "127.0.0.1:9000",
    [switch] $NoAuth,
    [switch] $RequireAuth,
    [string] $Url   = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$port = ($Bind -split ':')[-1]
if (-not $Url) { $Url = "ws://127.0.0.1:$port/ws" }

$root = $PSScriptRoot ? (Split-Path $PSScriptRoot) : $PWD
$pnpmScript = Join-Path $root "scripts\pnpm.ps1"

Write-Host "Launching backend server ($Bind) ..." -ForegroundColor Cyan
$serverArgs = @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $root "scripts\dev-server.ps1"), "-Bind", $Bind)
if ($NoAuth) { $serverArgs += "-NoAuth" }
if ($RequireAuth) { $serverArgs += "-RequireAuth" }

$serverProc = Start-Process pwsh -ArgumentList $serverArgs -PassThru

Write-Host "Waiting 3 s for backend to start ..." -ForegroundColor DarkYellow
Start-Sleep 3

Write-Host "Launching Web UI (backend: $Url) ..." -ForegroundColor Cyan
$env:VITE_WANLOGGER_URL = $Url
$webJob = Start-Job -ScriptBlock {
    param($r, $u, $pnpm)
    $env:VITE_WANLOGGER_URL = $u
    Set-Location $r
    & $pnpm --filter ./web dev
} -ArgumentList $root, $Url, $pnpmScript

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
