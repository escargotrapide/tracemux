# Convert non-UTF-8 source files (likely Shift-JIS) to UTF-8 (no BOM).
$ErrorActionPreference = 'Stop'
$strict = [System.Text.UTF8Encoding]::new($false, $true)
$utf8NoBom = [System.Text.UTF8Encoding]::new($false, $false)
$sjis = [System.Text.Encoding]::GetEncoding(932)

$converted = 0
$kept = 0
$paths = Get-ChildItem -Recurse -File -Path . -Include *.rs,*.ts,*.tsx,*.js,*.jsx,*.css,*.html,*.md,*.toml,*.yml,*.yaml,*.sh,*.ps1,*.json,*.jsonl,*.txt,justfile,clippy.toml,rustfmt.toml,deny.toml,cliff.toml `
    -ErrorAction SilentlyContinue `
    | Where-Object { $_.FullName -notmatch '\\target\\' -and $_.FullName -notmatch '\\.git\\' -and $_.FullName -notmatch '\\node_modules\\' -and $_.FullName -notmatch '\\dist\\' -and $_.FullName -notmatch '\\.vite\\' }

foreach ($f in $paths) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    if ($bytes.Length -eq 0) { continue }
    try {
        $null = $strict.GetString($bytes)
        $kept++
    } catch {
        # Try Shift-JIS -> UTF-8
        $text = $sjis.GetString($bytes)
        [System.IO.File]::WriteAllText($f.FullName, $text, $utf8NoBom)
        Write-Host "converted: $($f.FullName)"
        $converted++
    }
}
"converted=$converted kept=$kept"
