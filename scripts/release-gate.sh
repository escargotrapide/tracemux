#!/usr/bin/env bash
# Pre-release verification. Refuses to "release" while any of the
# following holds:
#
#   1. git working tree has uncommitted changes
#   2. workspace version is a -dev / -alpha / -rc / -beta string
#   3. CHANGELOG.md has no '## [X.Y.Z]' heading matching the version
#   4. the matching tag 'vX.Y.Z' does not yet exist locally
#   5. target/ai-verify.json is missing or has any failed step
#   6. 'cargo audit' and 'cargo deny check' do not pass
#
# Pass --allow-dev to skip checks (2) and (4) on a dev tree.
#
# REQ: FR-AI-002
set -uo pipefail

ALLOW="strict"
case "${1:-}" in
    --allow-dev) ALLOW="dev" ;;
esac

PROBLEMS=()
add_problem() { PROBLEMS+=("$1"); }

# 1. clean tree
if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
    add_problem "git working tree has uncommitted changes"
fi

# 2. version
VERSION=$(grep -E '^version\s*=\s*"' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
echo "release-gate: workspace version = $VERSION"
if [[ "$ALLOW" == "strict" ]] && [[ "$VERSION" =~ -(dev|alpha|beta|rc) ]]; then
    add_problem "workspace version $VERSION is not a stable release"
fi
CLEAN=${VERSION%%-*}

# 3. CHANGELOG entry
if [[ -f CHANGELOG.md ]]; then
    if ! grep -Eq "^##\s*\[?${CLEAN//./\\.}\]?" CHANGELOG.md; then
        add_problem "CHANGELOG.md has no entry for $CLEAN"
    fi
else
    add_problem "CHANGELOG.md is missing"
fi

# 4. tag exists
if [[ "$ALLOW" == "strict" ]]; then
    if ! git tag --list "v${CLEAN}" | grep -q .; then
        add_problem "git tag v${CLEAN} does not exist"
    fi
fi

# 5. ai-verify report
REPORT=target/ai-verify.json
if [[ ! -f "$REPORT" ]]; then
    add_problem "$REPORT missing -- run 'just ai-verify' first"
else
    if command -v python3 >/dev/null 2>&1; then
        FAILED=$(python3 -c "
import json,sys
ok={'pass','passed','ok','success','skip','skipped',''}
r=json.load(open('$REPORT'))
bad=[s.get('name','?') for s in r.get('steps',[]) if str(s.get('status','')).lower() not in ok]
print(','.join(bad))
")
        if [[ -n "$FAILED" ]]; then
            add_problem "ai-verify has failed step(s): $FAILED"
        fi
    fi
fi

# 6. audit + deny
for pair in 'cargo-audit:cargo audit' 'cargo-deny:cargo deny check'; do
    tool="${pair%%:*}"
    cmd="${pair#*:}"
    if ! command -v "$tool" >/dev/null 2>&1; then
        add_problem "$tool is not installed"
        continue
    fi
    echo "release-gate: running $cmd"
    if ! eval "$cmd" >/dev/null 2>&1; then
        add_problem "$cmd failed"
    fi
done

if [[ ${#PROBLEMS[@]} -eq 0 ]]; then
    echo "release-gate: green"
    exit 0
fi
echo "release-gate: ${#PROBLEMS[@]} blocker(s):"
for p in "${PROBLEMS[@]}"; do echo "  - $p"; done
exit ${#PROBLEMS[@]}
