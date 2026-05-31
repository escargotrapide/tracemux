#!/usr/bin/env pwsh
# Start the tracemux SolidJS web UI dev server.
# Usage: pwsh scripts/dev-web.ps1 [-Url <wss://...>] [-Token <bearer>]
#   -Url <uri>     Backend WS URL (default: ws://127.0.0.1:9000/ws)
#   -Token <str>   Bearer token (optional)
[CmdletBinding()]
param(
    [string] $Url   = "ws://127.0.0.1:9000/ws",
    [string] $Token = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$env:VITE_TRACEMUX_URL = $Url
if ($Token) { $env:VITE_TRACEMUX_TOKEN = $Token }

Write-Host "Starting Web UI dev server (backend: $Url)" -ForegroundColor Cyan
Write-Host "  Open: http://localhost:5173" -ForegroundColor DarkCyan
& (Join-Path $PSScriptRoot 'pnpm.ps1') --filter ./web dev
