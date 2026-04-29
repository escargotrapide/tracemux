#!/usr/bin/env pwsh
# Start the wanlogger SolidJS web UI dev server.
# Usage: pwsh scripts/dev-web.ps1 [-Url <wss://...>] [-Token <bearer>]
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

Write-Host "Starting Web UI dev server (backend: $Url)" -ForegroundColor Cyan
Write-Host "  Open: http://localhost:5173" -ForegroundColor DarkCyan
& pnpm --filter ./web dev
