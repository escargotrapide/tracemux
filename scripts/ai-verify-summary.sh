#!/usr/bin/env bash
# Run every AI-verify step in sequence, capture (name, status,
# duration_ms, detail), and emit target/ai-verify.json. Detail is the
# last 20 lines of stdout+stderr on failure; empty on success. Exit
# code is the number of failed steps (0 = green).
#
# REQ: FR-AI-001
set -uo pipefail

REPORT_PATH="${REPORT_PATH:-target/ai-verify.json}"
INCLUDE_OPTIONAL="${INCLUDE_OPTIONAL:-0}"
CI_SAFE_FEATURES="serial,metrics,desktop,headless"

mkdir -p "$(dirname "$REPORT_PATH")"

declare -a NAMES=(
  "encoding-check"
  "fmt-check"
  "clippy"
  "test"
  "rtm"
)
declare -a CMDS=(
  "bash scripts/check-encoding.sh"
  "cargo fmt --all -- --check"
  "cargo clippy --workspace --all-targets --features ${CI_SAFE_FEATURES} -- -D warnings"
  "cargo test --workspace --features ${CI_SAFE_FEATURES}"
  "bash scripts/gen-rtm.sh"
)

if [[ "$INCLUDE_OPTIONAL" == "1" ]]; then
  NAMES+=("web-typecheck" "web-test" "web-build")
  CMDS+=("pnpm --filter ./web typecheck" "pnpm --filter ./web test" "pnpm --filter ./web build")
fi

failed=0
json_steps=""
for i in "${!NAMES[@]}"; do
  name="${NAMES[$i]}"
  cmd="${CMDS[$i]}"
  start_ns=$(date +%s%N)
  set +e
  out=$(eval "$cmd" 2>&1)
  code=$?
  set -e
  end_ns=$(date +%s%N)
  dur_ms=$(( (end_ns - start_ns) / 1000000 ))
  if [[ $code -eq 0 ]]; then
    status="pass"
    detail="null"
  else
    status="fail"
    failed=$((failed + 1))
    tail_text=$(printf '%s' "$out" | tail -n 20 | tr -d '\r')
    # JSON-escape
    escaped=$(printf '%s' "$tail_text" | python3 -c 'import json,sys; sys.stdout.write(json.dumps(sys.stdin.read()))' 2>/dev/null \
      || printf '"%s"' "$(printf '%s' "$tail_text" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/\\t/g' | awk 'BEGIN{ORS="\\n"}1' | sed 's/\\n$//')")
    detail="$escaped"
  fi
  printf '[%-15s] %-4s %6d ms\n' "$name" "$status" "$dur_ms"
  json_steps+="{\"name\":\"$name\",\"status\":\"$status\",\"duration_ms\":$dur_ms,\"detail\":$detail}"
  if [[ $i -lt $((${#NAMES[@]} - 1)) ]]; then
    json_steps+=","
  fi
done

if [[ $failed -eq 0 ]]; then
  summary="green"
else
  summary="$failed failed"
fi
cat > "$REPORT_PATH" <<JSON
{"schema":"tracemux/ai-verify/v1","summary":"$summary","steps":[$json_steps]}
JSON
echo "Wrote $REPORT_PATH"
exit $failed
