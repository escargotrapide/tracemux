#!/usr/bin/env bash
# Start both the backend server and the web UI dev server together.
# Usage: bash scripts/dev-all.sh [--bind <h:p>] [--require-auth] [--url <ws://...>]
set -euo pipefail

BIND="127.0.0.1:9000"
NO_AUTH="--no-auth"
URL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bind)         BIND="$2"; shift 2 ;;
        --no-auth)      NO_AUTH="--no-auth"; shift ;;
        --require-auth) NO_AUTH=""; shift ;;
        --url)          URL="$2";  shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

PORT="${BIND##*:}"
[[ -z "$URL" ]] && URL="ws://127.0.0.1:${PORT}/ws"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cleanup() {
    echo ""
    echo "Stopping all..."
    kill "$SERVER_PID" 2>/dev/null || true
    kill "$WEB_PID"    2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "Launching backend server ($BIND) ..."
bash "$ROOT/scripts/dev-server.sh" --bind "$BIND" $NO_AUTH &
SERVER_PID=$!

echo "Waiting 3 s for backend to start ..."
sleep 3

echo "Launching Web UI (backend: $URL) ..."
export VITE_TRACEMUX_URL="$URL"
bash "$ROOT/scripts/dev-web.sh" --url "$URL" &
WEB_PID=$!

echo ""
echo "Both processes started."
echo "  Backend PID : $SERVER_PID"
echo "  Web UI  PID : $WEB_PID"
echo "  Web UI  URL : http://localhost:5173"
echo ""
echo "Press Ctrl+C to stop all."

wait
