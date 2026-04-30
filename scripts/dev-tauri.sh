#!/usr/bin/env bash
# Start the wanlogger Tauri desktop app in dev mode.
# Usage: bash scripts/dev-tauri.sh [--bind <h:p>] [--url <ws://...>] [--token <bearer>] [--no-sidecar]
set -euo pipefail

BIND="127.0.0.1:9000"
URL=""
TOKEN=""
SIDECAR="1"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bind)       BIND="$2"; shift 2 ;;
        --url)        URL="$2";   shift 2 ;;
        --token)      TOKEN="$2"; shift 2 ;;
        --no-sidecar) SIDECAR="0"; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

PORT="${BIND##*:}"
[[ -z "$URL" ]] && URL="ws://127.0.0.1:${PORT}/ws"

export VITE_WANLOGGER_URL="$URL"
[[ -n "$TOKEN" ]] && export VITE_WANLOGGER_TOKEN="$TOKEN"
export WANLOGGER_TAURI_BIND="$BIND"
export WANLOGGER_TAURI_SIDECAR="$SIDECAR"

echo "Starting Tauri desktop app (backend: $URL)"
if [[ "$SIDECAR" == "0" ]]; then
    echo "  Sidecar disabled; run dev-server.sh separately."
else
    echo "  Sidecar bind: $BIND"
fi
exec pnpm --filter ./app-tauri dev
