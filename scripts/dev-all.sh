#!/usr/bin/env bash
# Start both the backend server and the web UI dev server together.
# Usage: bash scripts/dev-all.sh [--port <n>] [--no-auth] [--url <wss://...>]
set -euo pipefail

PORT=9000
NO_AUTH=""
URL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --port)   PORT="$2";  shift 2 ;;
        --no-auth) NO_AUTH="--no-auth"; shift ;;
        --url)    URL="$2";   shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

[[ -z "$URL" ]] && URL="wss://localhost:${PORT}/ws"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cleanup() {
    echo ""
    echo "Stopping all..."
    kill "$SERVER_PID" 2>/dev/null || true
    kill "$WEB_PID"    2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "Launching backend server on port $PORT ..."
bash "$ROOT/scripts/dev-server.sh" --port "$PORT" $NO_AUTH &
SERVER_PID=$!

echo "Waiting 3 s for backend to start ..."
sleep 3

echo "Launching Web UI (backend: $URL) ..."
export VITE_WANLOGGER_URL="$URL"
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
