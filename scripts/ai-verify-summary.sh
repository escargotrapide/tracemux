#!/usr/bin/env bash
# Aggregate the AI verification results into target/ai-verify.json.
# v0.1 stub. Real impl reads each step's exit code and produces a
# stable JSON document consumed by the server's /api/ai/verify.
set -euo pipefail

mkdir -p target
cat > target/ai-verify.json <<'JSON'
{
  "schema": "wanlogger/ai-verify/v1",
  "summary": "stub",
  "steps": []
}
JSON
echo "Wrote target/ai-verify.json"
