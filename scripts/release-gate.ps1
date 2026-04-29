# Pre-release verification. Refuses to "release" while any of the
# following holds:
#
#   1. git working tree has uncommitted changes
#   2. workspace version is a -dev / -alpha / -rc / -beta string
#      (release builds must use a stable semver)
#   3. CHANGELOG.md has no `## [X.Y.Z]` heading matching the version
#   4. the matching tag `vX.Y.Z` does not yet exist locally
#   5. `target/ai-verify.json` is missing or has any failed step
#   6. `cargo audit` and `cargo deny check` do not pass
#
# Pass `-Allow Dev` to skip check (2) and (4) when running on a dev tree.
#
# REQ: FR-AI-002
[CmdletBinding()]
param(
    [ValidateSet('Strict', 'Dev')]
    [string]$Allow = 'Strict'
)

$ErrorActionPreference = 'Stop'
$problems = New-Object System.Collections.Generic.List[string]

function Add-Problem($msg) { $problems.Add($msg) | Out-Null }

# 1. clean tree
$dirty = git status --porcelain
if ($dirty) {
    Add-Problem "git working tree has uncommitted changes"
}

# 2. version
$version = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "release-gate: workspace version = $version"
if ($Allow -eq 'Strict' -and $version -match '-(dev|alpha|beta|rc)') {
    Add-Problem "workspace version $version is not a stable release"
}

# 3. CHANGELOG entry
if (Test-Path 'CHANGELOG.md') {
    $clean = $version -replace '-.*$', ''
    $found = Select-String -Path 'CHANGELOG.md' -Pattern "^##\s*\[?$([regex]::Escape($clean))\]?"
    if (-not $found) {
        Add-Problem "CHANGELOG.md has no entry for $clean"
    }
} else {
    Add-Problem "CHANGELOG.md is missing"
}

# 4. tag exists
if ($Allow -eq 'Strict') {
    $clean = $version -replace '-.*$', ''
    $tag = "v$clean"
    $hasTag = git tag --list $tag
    if (-not $hasTag) {
        Add-Problem "git tag $tag does not exist"
    }
}

# 5. ai-verify report
$report = 'target/ai-verify.json'
if (-not (Test-Path $report)) {
    Add-Problem "$report missing -- run 'just ai-verify' first"
} else {
    try {
        $j = Get-Content -Raw $report | ConvertFrom-Json
        $failed = @($j.steps | Where-Object {
                $_.status -notin @('pass', 'passed', 'ok', 'success', 'skip', 'skipped', '')
            })
        if ($failed.Count -gt 0) {
            $names = ($failed | ForEach-Object { $_.name }) -join ', '
            Add-Problem "ai-verify has $($failed.Count) failed step(s): $names"
        }
    } catch {
        Add-Problem "$report is not valid JSON"
    }
}

# 6. audit + deny (best-effort: missing tool is also a problem)
foreach ($pair in @(@('cargo-audit', 'cargo audit'), @('cargo-deny', 'cargo deny check'))) {
    $tool = $pair[0]
    $cmd = $pair[1]
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Add-Problem "$tool is not installed"
        continue
    }
    Write-Host "release-gate: running $cmd"
    $out = & cmd /c "$cmd 2>&1"
    if ($LASTEXITCODE -ne 0) {
        Add-Problem "$cmd failed (exit $LASTEXITCODE)"
    }
}

if ($problems.Count -eq 0) {
    Write-Host "release-gate: green"
    exit 0
}
Write-Host "release-gate: $($problems.Count) blocker(s):"
foreach ($p in $problems) { Write-Host "  - $p" }
exit $problems.Count
