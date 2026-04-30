#!/usr/bin/env pwsh
# Start the wanlogger Tauri desktop app in dev mode.
# The Tauri dev setup starts its own Vite dev server internally.
# Usage: pwsh scripts/dev-tauri.ps1 [-Bind <host:port>] [-Url <ws://...>] [-Token <bearer>] [-NoSidecar]
#   -Bind <h:p>    Sidecar bind address (default 127.0.0.1:9000)
#   -Url <uri>     Backend WS URL (default: ws://127.0.0.1:<port>/ws)
#   -Token <str>   Bearer token (optional; not needed for loopback sidecar)
#   -NoSidecar     Do not spawn the bundled sidecar
[CmdletBinding()]
param(
    [string] $Bind  = "127.0.0.1:9000",
    [string] $Url   = "",
    [string] $Token = "",
    [switch] $NoSidecar
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$port = ($Bind -split ':')[-1]
if (-not $Url) { $Url = "ws://127.0.0.1:$port/ws" }

$env:VITE_WANLOGGER_URL = $Url
if ($Token) { $env:VITE_WANLOGGER_TOKEN = $Token }
$env:WANLOGGER_TAURI_BIND = $Bind
$env:WANLOGGER_TAURI_SIDECAR = if ($NoSidecar) { "0" } else { "1" }

Write-Host "Starting Tauri desktop app (backend: $Url)" -ForegroundColor Cyan
if ($NoSidecar) {
    Write-Host "  Sidecar disabled; run dev-server.ps1 separately." -ForegroundColor DarkYellow
} else {
    Write-Host "  Sidecar bind: $Bind" -ForegroundColor DarkCyan
}
& pnpm --filter ./app-tauri dev
