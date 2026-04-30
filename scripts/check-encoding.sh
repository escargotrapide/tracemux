#!/usr/bin/env bash
# Verify all tracked text files are valid UTF-8.
# Linux/macOS equivalent of scripts/check-encoding.ps1.
# Requires: iconv (standard on Linux/macOS).
set -euo pipefail

bad=()

while IFS= read -r -d '' f; do
    iconv -f UTF-8 -t UTF-8 "$f" >/dev/null 2>&1 || bad+=("$f")
done < <(find . \
     \( -path './.git' -o -path '*/target' -o -path '*/node_modules' \
         -o -path '*/dist' -o -path '*/.vite' \
    \) -prune \
    -o -type f \
     \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' \
         -o -name '*.jsx' -o -name '*.css' -o -name '*.html' \
         -o -name '*.md' -o -name '*.toml' -o -name '*.yml' \
         -o -name '*.yaml' -o -name '*.sh' -o -name '*.ps1' -o -name '*.json' \
         -o -name '*.jsonl' -o -name '*.txt' \
       -o -name 'justfile' -o -name 'clippy.toml' -o -name 'rustfmt.toml' \
       -o -name 'deny.toml' -o -name 'cliff.toml' -o -name '.gitattributes' \
       -o -name '.editorconfig' -o -name '.gitignore' \
    \) -print0)

count=$(find . \
     \( -path './.git' -o -path '*/target' -o -path '*/node_modules' \
         -o -path '*/dist' -o -path '*/.vite' \
    \) -prune \
    -o -type f \
     \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' \
         -o -name '*.jsx' -o -name '*.css' -o -name '*.html' \
         -o -name '*.md' -o -name '*.toml' -o -name '*.yml' \
         -o -name '*.yaml' -o -name '*.sh' -o -name '*.ps1' -o -name '*.json' \
         -o -name '*.jsonl' -o -name '*.txt' \
       -o -name 'justfile' -o -name 'clippy.toml' -o -name 'rustfmt.toml' \
       -o -name 'deny.toml' -o -name 'cliff.toml' -o -name '.gitattributes' \
       -o -name '.editorconfig' -o -name '.gitignore' \
    \) -print | wc -l | tr -d ' ')

if [ "${#bad[@]}" -gt 0 ]; then
    echo "ERROR: ${#bad[@]} file(s) are not valid UTF-8:" >&2
    printf '  %s\n' "${bad[@]}" >&2
    echo "Run scripts/fix-encoding.sh to attempt a Shift-JIS -> UTF-8 conversion." >&2
    exit 1
fi

echo "OK: all ${count} text files are valid UTF-8."
