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
        REPORT_PROBLEM=$(python3 - "$REPORT" <<'PY'
import json
import sys

path = sys.argv[1]
required = ["encoding-check", "fmt-check", "clippy", "test", "rtm"]
pass_required = {"pass", "passed", "ok", "success"}
pass_any = pass_required | {"skip", "skipped"}
problems = []

try:
    with open(path, encoding="utf-8") as f:
        report = json.load(f)
except Exception as exc:  # noqa: BLE001 - shell gate reports the message.
    print(f"not valid JSON: {exc}")
    sys.exit(0)

if report.get("schema") != "wanlogger/ai-verify/v1":
    problems.append("schema is not wanlogger/ai-verify/v1")
if report.get("summary") != "green":
    problems.append(f"summary is {report.get('summary')!r}, expected 'green'")

steps = report.get("steps")
if not isinstance(steps, list) or not steps:
    problems.append("steps is empty")
    steps = []

by_name = {}
for step in steps:
    if not isinstance(step, dict):
        problems.append("step entry is not an object")
        continue
    name = str(step.get("name", ""))
    status = str(step.get("status", "")).lower()
    by_name[name] = status
    if status not in pass_any:
        problems.append(f"step {name or '?'} has status {status or '<empty>'}")

for name in required:
    status = by_name.get(name)
    if status is None:
        problems.append(f"required step {name} missing")
    elif status not in pass_required:
        problems.append(f"required step {name} has status {status or '<empty>'}")

print("; ".join(problems))
PY
)
        if [[ -n "$REPORT_PROBLEM" ]]; then
            add_problem "ai-verify report invalid: $REPORT_PROBLEM"
        fi
    else
        add_problem "python3 is required to validate $REPORT"
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
