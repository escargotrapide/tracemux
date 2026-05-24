# Run every AI-verify step in sequence, capture (name, status,
# duration_ms, detail), and emit target/ai-verify.json.
#
# Detail is the last 20 lines of stdout+stderr on failure; empty on
# success.  Exit code is the number of failed steps (0 = green).
#
# Steps are defined inline below so this script does not depend on
# YAML/TOML parsing.  The Rust side (`wanlogger ai-verify`) only
# reads the JSON.
#
# REQ: FR-AI-001
[CmdletBinding()]
param(
    [string]$ReportPath = 'target/ai-verify.json',
    [switch]$IncludeOptional
)

$ErrorActionPreference = 'Continue'

# Keep the default gate runnable on machines without host packet-capture SDKs.
# `pcap-capture` is verified explicitly on configured environments because it
# links Npcap/libpcap (`wpcap.lib` on Windows).
$ciSafeFeatures = 'serial,metrics,desktop,headless'

$steps = @(
    @{ name = 'encoding-check'; cmd = 'pwsh'; words = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/check-encoding.ps1') }
    @{ name = 'fmt-check';      cmd = 'cargo'; words = @('fmt', '--all', '--', '--check') }
    @{ name = 'clippy';         cmd = 'cargo'; words = @('clippy', '--workspace', '--all-targets', '--features', $ciSafeFeatures, '--', '-D', 'warnings') }
    @{ name = 'test';           cmd = 'cargo'; words = @('test', '--workspace', '--features', $ciSafeFeatures) }
    @{ name = 'rtm';            cmd = 'pwsh'; words = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/gen-rtm.ps1') }
)
if ($IncludeOptional) {
    $steps += @{ name = 'web-typecheck'; cmd = 'pnpm.cmd'; words = @('--filter', './web', 'typecheck') }
    $steps += @{ name = 'web-test';      cmd = 'pnpm.cmd'; words = @('--filter', './web', 'test') }
    $steps += @{ name = 'web-build';     cmd = 'pnpm.cmd'; words = @('--filter', './web', 'build') }
}

function Invoke-VerifyStep {
    param(
        [Parameter(Mandatory = $true)] [string]$CommandName,
        [Parameter(Mandatory = $true)] [string[]]$Words,
        [Parameter(Mandatory = $true)] [string]$StdOutPath,
        [Parameter(Mandatory = $true)] [string]$StdErrPath
    )
    & $CommandName @Words 1> $StdOutPath 2> $StdErrPath
}

$results = @()
$failed = 0

foreach ($s in $steps) {
    $name = $s.name
    $cmd  = $s.cmd
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $tmpOut = [System.IO.Path]::GetTempFileName()
    $tmpErr = [System.IO.Path]::GetTempFileName()
    try {
        Invoke-VerifyStep -CommandName $cmd -Words ([string[]]$s.words) -StdOutPath $tmpOut -StdErrPath $tmpErr
        $code = if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE }
    } catch {
        $code = 1
        $_.Exception.Message | Out-File -FilePath $tmpErr -Append -Encoding utf8
    }
    $sw.Stop()
    $stdout = (Get-Content -Raw -ErrorAction SilentlyContinue -Path $tmpOut)
    $stderr = (Get-Content -Raw -ErrorAction SilentlyContinue -Path $tmpErr)
    Remove-Item -Force $tmpOut, $tmpErr -ErrorAction SilentlyContinue

    $status = if ($code -eq 0) { 'pass' } else { 'fail' }
    $detail = $null
    if ($code -ne 0) {
        $combined = (("$stdout`n$stderr") -split "`n")
        $tail = $combined | Select-Object -Last 20
        $detail = ($tail -join "`n").Trim()
        $failed += 1
    }

    $results += [ordered]@{
        name        = $name
        status      = $status
        duration_ms = [int]$sw.ElapsedMilliseconds
        detail      = $detail
    }
    Write-Host ("[{0,-15}] {1,-5} {2,6} ms" -f $name, $status, $sw.ElapsedMilliseconds)
}

$report = [ordered]@{
    schema  = 'wanlogger/ai-verify/v1'
    summary = if ($failed -eq 0) { 'green' } else { "$failed failed" }
    steps   = $results
}

$null = New-Item -ItemType Directory -Force -Path (Split-Path $ReportPath -Parent)
$json = $report | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText(
    (Resolve-Path -LiteralPath (Split-Path $ReportPath -Parent) | Join-Path -ChildPath (Split-Path $ReportPath -Leaf)),
    $json,
    [System.Text.UTF8Encoding]::new($false))
Write-Host "Wrote $ReportPath"

exit $failed
