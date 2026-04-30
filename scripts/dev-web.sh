#!/usr/bin/env bash
# Start the wanlogger SolidJS web UI dev server.
# Usage: bash scripts/dev-web.sh [--url <wss://...>] [--token <bearer>]
set -euo pipefail

URL="ws://127.0.0.1:9000/ws"
TOKEN=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --url)   URL="$2";   shift 2 ;;
        --token) TOKEN="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

export VITE_WANLOGGER_URL="$URL"
[[ -n "$TOKEN" ]] && export VITE_WANLOGGER_TOKEN="$TOKEN"

echo "Starting Web UI dev server (backend: $URL)"
echo "  Open: http://localhost:5173"
exec pnpm --filter ./web dev
