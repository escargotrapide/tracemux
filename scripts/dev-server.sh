#!/usr/bin/env bash
# Start the wanlogger backend server (development mode).
# Usage: bash scripts/dev-server.sh [options]
#   --port <n>    Listen port (default 9000)
#   --no-auth     Disable auth (loopback only)
#   --release     Build in release mode
set -euo pipefail

PORT=9000
NO_AUTH=""
BUILD_FLAG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --port)    PORT="$2"; shift 2 ;;
        --no-auth) NO_AUTH="--no-auth"; shift ;;
        --release) BUILD_FLAG="--release"; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

CARGO_ARGS=(run $BUILD_FLAG -p wanlogger-cli -- serve --port "$PORT" $NO_AUTH)
echo "Starting server: cargo ${CARGO_ARGS[*]}"
exec cargo "${CARGO_ARGS[@]}"
