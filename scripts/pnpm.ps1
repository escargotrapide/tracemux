param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments
)

$direct = Get-Command pnpm.cmd -ErrorAction SilentlyContinue
if (-not $direct) { $direct = Get-Command pnpm -ErrorAction SilentlyContinue }
if ($direct) {
    & $direct.Source @Arguments
    exit $LASTEXITCODE
}

$corepack = Get-Command corepack.cmd -ErrorAction SilentlyContinue
if (-not $corepack) { $corepack = Get-Command corepack -ErrorAction SilentlyContinue }
if ($corepack) {
    $corepackArgs = @('pnpm') + $Arguments
    & $corepack.Source @corepackArgs
    exit $LASTEXITCODE
}

Write-Error 'pnpm is required. Install pnpm, or install Node.js with Corepack enabled.'
exit 127