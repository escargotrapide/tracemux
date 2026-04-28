# Aggregate the AI verification results into target/ai-verify.json.
# Windows equivalent of scripts/ai-verify-summary.sh. v0.1 stub.
$ErrorActionPreference = 'Stop'

$null = New-Item -ItemType Directory -Force -Path 'target'
$json = @'
{
  "schema": "wanlogger/ai-verify/v1",
  "summary": "stub",
  "steps": []
}
'@
$fullOut = Join-Path (Get-Location) 'target/ai-verify.json'
[System.IO.File]::WriteAllText($fullOut, $json, [System.Text.UTF8Encoding]::new($false))
Write-Host 'Wrote target/ai-verify.json'
