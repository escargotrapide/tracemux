#!/usr/bin/env bash
# Convert non-UTF-8 files (likely Shift-JIS / CP932) to UTF-8 (no BOM).
# Linux/macOS equivalent of scripts/fix-encoding.ps1.
# Requires: iconv (standard on Linux/macOS).
set -euo pipefail

converted=0
kept=0

while IFS= read -r -d '' f; do
    if iconv -f UTF-8 -t UTF-8 "$f" >/dev/null 2>&1; then
        kept=$((kept + 1))
    else
        tmp=$(mktemp)
        if iconv -f CP932 -t UTF-8 "$f" >"$tmp" 2>/dev/null; then
            mv "$tmp" "$f"
            echo "converted: $f"
            converted=$((converted + 1))
        else
            rm -f "$tmp"
            echo "WARN: could not convert $f (tried CP932)" >&2
            kept=$((kept + 1))
        fi
    fi
done < <(find . \
     \( -path './.git' -o -path '*/target' -o -path '*/node_modules' \
         -o -path '*/dist' -o -path '*/.vite' \
    \) -prune \
    -o -type f \
     \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' \
         -o -name '*.jsx' -o -name '*.css' -o -name '*.html' \
         -o -name '*.md' -o -name '*.toml' -o -name '*.yml' \
         -o -name '*.yaml' -o -name '*.sh' -o -name '*.ps1' -o -name '*.json' \
         -o -name '*.jsonl' \
         -o -name '*.txt' -o -name 'justfile' \
       -o -name 'clippy.toml' -o -name 'rustfmt.toml' -o -name 'deny.toml' \
       -o -name 'cliff.toml' \
    \) -print0)

echo "converted=${converted} kept=${kept}"
