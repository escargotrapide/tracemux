#!/usr/bin/env pwsh
# Start the wanlogger Tauri desktop app in dev mode.
# The Tauri dev setup starts its own Vite dev server internally.
# Usage: pwsh scripts/dev-tauri.ps1 [-Url <wss://...>] [-Token <bearer>]
#   -Url <uri>     Backend WS URL (default: wss://localhost:9000/ws)
#   -Token <str>   Bearer token (optional)
[CmdletBinding()]
param(
    [string] $Url   = "wss://localhost:9000/ws",
    [string] $Token = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$env:VITE_WANLOGGER_URL = $Url
if ($Token) { $env:VITE_WANLOGGER_TOKEN = $Token }

Write-Host "Starting Tauri desktop app (backend: $Url)" -ForegroundColor Cyan
Write-Host "  Note: run dev-server.ps1 in a separate terminal first." -ForegroundColor DarkYellow
& pnpm --filter ./app-tauri dev
