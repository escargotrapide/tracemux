#!/usr/bin/env pwsh
# Start backend server, web UI, and virtual peer together (development mode).
#
# The virtual peer listens on a TCP port so tracemux can connect to it as a
# `tcp` source.  Once all three processes are running, open the UI at
# http://localhost:5173, add a TCP source pointing to 127.0.0.1:<PeerPort>,
# and you will see the scripted device traffic flow in.
#
# Usage: pwsh scripts/dev-all-with-peer.ps1 [options]
#   -Bind <h:p>       Backend bind address     (default 127.0.0.1:9000)
#   -PeerPort <port>  Virtual-peer listen port (default 9001)
#   -PeerSend <text>  Text the peer sends      (default "Hello from virt-peer")
#   -RepeatCount <n>  How many times to repeat (default 3600)
#   -IntervalMs <ms>  Interval between sends   (default 2000)
#   -RequireAuth      Do not pass --no-auth to the server
#   -Url <uri>        Override WS URL for the web UI
[CmdletBinding()]
param(
    [string] $Bind        = "127.0.0.1:9000",
    [int]    $PeerPort    = 9001,
    [string] $PeerSend    = "Hello from virt-peer",
    [int]    $RepeatCount = 3600,
    [int]    $IntervalMs  = 2000,
    [switch] $RequireAuth,
    [string] $Url         = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$port = ($Bind -split ':')[-1]
if (-not $Url) { $Url = "ws://127.0.0.1:$port/ws" }

$root = $PSScriptRoot ? (Split-Path $PSScriptRoot) : $PWD

# ---- 1. Backend server -------------------------------------------------------
Write-Host "Launching backend server ($Bind) ..." -ForegroundColor Cyan
$serverArgs = @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $root "scripts\dev-server.ps1"), "-Bind", $Bind)
if (-not $RequireAuth) { $serverArgs += "-NoAuth" }
$serverProc = Start-Process pwsh -ArgumentList $serverArgs -PassThru

Write-Host "Waiting 3 s for backend to start ..." -ForegroundColor DarkYellow
Start-Sleep 3

# ---- 2. Virtual peer ---------------------------------------------------------
$peerAddr = "127.0.0.1:$PeerPort"
Write-Host "Launching virtual peer (TCP listen $peerAddr) ..." -ForegroundColor Cyan
$peerJob = Start-Job -ScriptBlock {
    param($r, $addr, $send, $repeat, $interval)
    Set-Location $r
    & cargo run -p tracemux-virt-peer -- `
        tcp --mode listen --addr $addr `
        --send $send --eol lf `
        --repeat $repeat `
        --interval-ms $interval
} -ArgumentList $root, $peerAddr, $PeerSend, $RepeatCount, $IntervalMs

# ---- 3. Web UI ---------------------------------------------------------------
Write-Host "Launching Web UI (backend: $Url) ..." -ForegroundColor Cyan
$env:VITE_TRACEMUX_URL = $Url
# Prefer the direct `pnpm` command; fall back to `corepack pnpm` if not in PATH.
$pnpmCmd = if (Get-Command pnpm -ErrorAction SilentlyContinue) { "pnpm" } else { $null }
$webJob = Start-Job -ScriptBlock {
    param($r, $u, $pnpm)
    $env:VITE_TRACEMUX_URL = $u
    Set-Location $r
    if ($pnpm) {
        & $pnpm --filter ./web dev
    } else {
        & corepack pnpm --filter ./web dev
    }
} -ArgumentList $root, $Url, $pnpmCmd

Write-Host ""
Write-Host "All three processes started." -ForegroundColor Green
Write-Host "  Backend PID  : $($serverProc.Id)"
Write-Host "  Virtual peer : TCP listen $peerAddr  (Job $($peerJob.Id))"
Write-Host "  Web UI       : http://localhost:5173  (Job $($webJob.Id))"
Write-Host ""
Write-Host "  Connect tracemux to the virtual peer:" -ForegroundColor Yellow
Write-Host "    Source URL: tcp://127.0.0.1:$PeerPort" -ForegroundColor Yellow
Write-Host ""
Write-Host "Press Ctrl+C to stop all." -ForegroundColor DarkYellow

try {
    while ($true) {
        $webOut  = Receive-Job -Job $webJob  -ErrorAction SilentlyContinue
        $peerOut = Receive-Job -Job $peerJob -ErrorAction SilentlyContinue
        if ($webOut)  { Write-Host "[web]  $webOut" }
        if ($peerOut) { Write-Host "[peer] $peerOut" }
        if ($serverProc.HasExited) {
            Write-Warning "Backend process exited (code $($serverProc.ExitCode))."
            break
        }
        Start-Sleep -Milliseconds 500
    }
} finally {
    Stop-Job  -Job $webJob  -ErrorAction SilentlyContinue
    Remove-Job -Job $webJob  -Force -ErrorAction SilentlyContinue
    Stop-Job  -Job $peerJob -ErrorAction SilentlyContinue
    Remove-Job -Job $peerJob -Force -ErrorAction SilentlyContinue
    if (-not $serverProc.HasExited) {
        Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue
    }
    Write-Host "Stopped." -ForegroundColor DarkYellow
}
