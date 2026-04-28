# wanlogger development tasks
#
# Install: https://github.com/casey/just
# Run `just` to list tasks.

set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]
set shell := ["bash", "-cu"]

default:
    @just --list

# ---- Build ----------------------------------------------------------------

build:
    cargo build --workspace --all-targets

build-release:
    cargo build --workspace --release

# ---- Format / Lint --------------------------------------------------------

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Reject non-UTF-8 / CRLF text files (see AGENTS.md pitfalls).
[windows]
encoding-check:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/check-encoding.ps1

[unix]
encoding-check:
    bash scripts/check-encoding.sh

[windows]
encoding-fix:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/fix-encoding.ps1

[unix]
encoding-fix:
    bash scripts/fix-encoding.sh

# ---- Test -----------------------------------------------------------------

test:
    cargo test --workspace --all-features

# ---- Security -------------------------------------------------------------

audit:
    cargo audit

deny:
    cargo deny check

# ---- Coverage / Bench / Mutants / Fuzz ------------------------------------

coverage:
    cargo llvm-cov --workspace --lcov --output-path target/lcov.info

bench:
    cargo bench --workspace

bench-baseline:
    cargo bench --workspace -- --save-baseline baseline

mutants:
    cargo mutants --workspace --in-place

fuzz-smoke:
    @echo "Smoke-fuzzing each target for 60s"
    cargo +nightly fuzz run --release telnet_iac -- -max_total_time=60 || true
    cargo +nightly fuzz run --release vt_escape  -- -max_total_time=60 || true
    cargo +nightly fuzz run --release index_jsonl -- -max_total_time=60 || true
    cargo +nightly fuzz run --release wire_proto -- -max_total_time=60 || true
    cargo +nightly fuzz run --release framer    -- -max_total_time=60 || true
    cargo +nightly fuzz run --release decoder   -- -max_total_time=60 || true

# ---- Docs / Schema / RTM --------------------------------------------------

docs:
    cargo doc --workspace --no-deps

schema:
    cargo run -p wanlogger-cli -- json-schema --out docs/protocols/cli-output/v1/

[windows]
rtm:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/gen-rtm.ps1

[unix]
rtm:
    bash scripts/gen-rtm.sh

# ---- E2E ------------------------------------------------------------------

e2e:
    pnpm --filter ./web e2e

# ---- Internal helpers (platform-split) ------------------------------------

[windows]
_verify-summary:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/ai-verify-summary.ps1

[unix]
_verify-summary:
    bash scripts/ai-verify-summary.sh

# ---- Aggregate AI verification gate ---------------------------------------
# Outputs JSON summary to target/ai-verify.json. Used by /api/ai/verify.
# Heavy steps (audit/deny/coverage/bench/fuzz) are gated on tool presence so
# this recipe also runs locally on a fresh checkout. CI runs the full set.
ai-verify:
    just encoding-check
    just fmt-check
    just clippy
    just test
    just _verify-summary

# Full verification including optional tooling (CI-only by default).
ai-verify-full:
    just encoding-check
    just fmt-check
    just clippy
    just test
    just audit
    just deny
    just coverage
    just bench
    just fuzz-smoke
    just rtm
    just _verify-summary

# ---- Release --------------------------------------------------------------

[windows]
release-gate:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/release-gate.ps1

[unix]
release-gate:
    bash scripts/release-gate.sh

# ---- Tauri / Web ----------------------------------------------------------

web-dev:
    pnpm --filter ./web dev

web-build:
    pnpm --filter ./web build

tauri-dev:
    cargo tauri dev

tauri-build:
    cargo tauri build
