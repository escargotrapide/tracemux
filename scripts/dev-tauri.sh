#!/usr/bin/env bash
# Start the wanlogger Tauri desktop app in dev mode.
# Usage: bash scripts/dev-tauri.sh [--url <wss://...>] [--token <bearer>]
set -euo pipefail

URL="wss://localhost:9000/ws"
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

echo "Starting Tauri desktop app (backend: $URL)"
echo "  Note: run dev-server.sh in a separate terminal first."
exec pnpm --filter ./app-tauri dev
