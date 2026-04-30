# Verify all tracked text files are valid UTF-8.
# Exits 0 if all files are UTF-8, 1 otherwise. Lists offending files.
$ErrorActionPreference = 'Stop'
$strict = [System.Text.UTF8Encoding]::new($false, $true)
$bad = @()
$paths = Get-ChildItem -Recurse -File -Path . `
    -Include *.rs,*.ts,*.tsx,*.js,*.jsx,*.css,*.html,*.md,*.toml,*.yml,*.yaml,*.sh,*.ps1,*.json,*.jsonl,*.txt,justfile,clippy.toml,rustfmt.toml,deny.toml,cliff.toml,.gitattributes,.editorconfig,.gitignore `
    | Where-Object { $_.FullName -notmatch '\\target\\' -and $_.FullName -notmatch '\\.git\\' -and $_.FullName -notmatch '\\node_modules\\' -and $_.FullName -notmatch '\\dist\\' -and $_.FullName -notmatch '\\.vite\\' }
foreach ($f in $paths) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    if ($bytes.Length -eq 0) { continue }
    try { $null = $strict.GetString($bytes) } catch { $bad += $f.FullName }
}
if ($bad.Count -gt 0) {
    Write-Host "ERROR: $($bad.Count) file(s) are not valid UTF-8:" -ForegroundColor Red
    $bad | ForEach-Object { Write-Host "  $_" }
    Write-Host "Run scripts/fix-encoding.ps1 to attempt a Shift-JIS -> UTF-8 conversion." -ForegroundColor Yellow
    exit 1
}
Write-Host "OK: all $($paths.Count) text files are valid UTF-8." -ForegroundColor Green
exit 0
