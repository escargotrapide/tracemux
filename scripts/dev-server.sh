#!/usr/bin/env bash
# Start the tracemux backend server (development mode).
# Usage: bash scripts/dev-server.sh [options]
#   --bind <h:p>       Bind address (default 127.0.0.1:9000)
#   --require-auth     Do not pass --no-auth (default: loopback --no-auth)
#   --no-auth          Accepted for compatibility; loopback --no-auth is the default
#   --release          Build in release mode
set -euo pipefail

BIND="127.0.0.1:9000"
NO_AUTH="--no-auth"
BUILD_FLAG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bind)         BIND="$2"; shift 2 ;;
        --no-auth)      NO_AUTH="--no-auth"; shift ;;
        --require-auth) NO_AUTH=""; shift ;;
        --release)      BUILD_FLAG="--release"; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

CARGO_ARGS=(run $BUILD_FLAG -p tracemux-cli -- serve --bind "$BIND" $NO_AUTH)
echo "Starting server: cargo ${CARGO_ARGS[*]}"
exec cargo "${CARGO_ARGS[@]}"
