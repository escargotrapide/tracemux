# tracemux development tasks
#
# Install: https://github.com/casey/just
# Run `just` to list tasks.

set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]
set shell := ["bash", "-cu"]
ci-safe-features := "serial,metrics,desktop,headless"

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
    cargo clippy --workspace --all-targets --features {{ci-safe-features}} -- -D warnings

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
    cargo test --workspace --features {{ci-safe-features}}

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
    cargo run -p tracemux-cli -- json-schema --out docs/protocols/cli-output/v1/

[windows]
rtm:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/gen-rtm.ps1

[unix]
rtm:
    bash scripts/gen-rtm.sh

# ---- E2E ------------------------------------------------------------------

[windows]
e2e:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/pnpm.ps1 --filter ./web e2e

[unix]
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
# The driver script in scripts/ai-verify-summary.{ps1,sh} runs each step,
# captures status/duration/last-20-lines-of-output, and writes
# target/ai-verify.json. Exit code = number of failed steps (0 = green).
ai-verify:
    just _verify-summary

# Full verification including web + optional tooling.
[windows]
ai-verify-full:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/ai-verify-summary.ps1 -IncludeOptional

[unix]
ai-verify-full:
    INCLUDE_OPTIONAL=1 bash scripts/ai-verify-summary.sh

# ---- Dev launch (GUI) -----------------------------------------------------

# Prepare Tauri dev env: build CLI sidecar + generate placeholder icons.
# Run once before dev-tauri. Re-run after rebuilding tracemux-cli.
[windows]
dev-prepare:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-prepare.ps1

[unix]
dev-prepare:
    bash scripts/dev-prepare.sh

# Start the backend server only (dev mode, bind 127.0.0.1:9000).
[windows]
dev-server:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-server.ps1

[unix]
dev-server:
    bash scripts/dev-server.sh

# Start the SolidJS web UI dev server only (assumes backend is running).
[windows]
dev-web:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-web.ps1

[unix]
dev-web:
    bash scripts/dev-web.sh

# Start the Tauri desktop app in dev mode (assumes backend is running).
[windows]
dev-tauri:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-tauri.ps1

[unix]
dev-tauri:
    bash scripts/dev-tauri.sh

# Start both backend + web UI together (Ctrl+C stops both).
[windows]
dev-all:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-all.ps1

[unix]
dev-all:
    bash scripts/dev-all.sh

# Start backend + web UI + virtual peer together (TCP listen on port 9001).
# Connect tracemux to tcp://127.0.0.1:9001 via the UI to see device traffic.
[windows]
dev-all-with-peer:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/dev-all-with-peer.ps1

# ---- Release --------------------------------------------------------------

[windows]
release-gate:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/release-gate.ps1

[unix]
release-gate:
    bash scripts/release-gate.sh

# ---- Tauri / Web ----------------------------------------------------------

[windows]
web-dev:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/pnpm.ps1 --filter ./web dev

[unix]
web-dev:
    pnpm --filter ./web dev

[windows]
web-build:
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/pnpm.ps1 --filter ./web build

[unix]
web-build:
    pnpm --filter ./web build

tauri-dev:
    cargo tauri dev

tauri-build:
    cargo tauri build
