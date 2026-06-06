# AGENTS.md — TraceMux

> Central instructions for AI coding agents (GitHub Copilot, Claude, Cursor, etc.)
> working in this repository. Human contributors should read it too.

This file is the **single source of truth** for:

- Where things live (directory map)
- Which paths are *critical* (require human review)
- How to build / test / lint / verify
- Project-wide invariants and pitfalls
- The layered architecture and frozen API surfaces

If you change architecture, **update this file in the same PR**.

---

## 1. Project mission

TraceMux is *"that one unified terminal to view and maintain all the logs,
either local or over networks"*. It is a lightweight, high-functionality,
multi-connection debug terminal + log platform.

Non-goals (v0.1): replacing Wireshark, replacing tail aggregators (Loki etc.),
shipping cloud SaaS.

## 2. Architecture in one screen

Four-layer abstraction (each layer is a frozen v0.1 trait):

```
  Source → Framer → Decoder → LogSink / UI
   (raw bytes / datagrams / events / opaque streams)
```

Plus orthogonal: **Sink** (write-back), **Importer / Exporter**,
**TimeseriesSink**, **TimeSource**.

Process layout:

```
  [browser / Tauri shell]  ──WSS (subprotocol "tracemux.v1", MessagePack)──▶  [tracemux serve]
                                                                                  │
                                                                                  ▼
                                                          Source registry → Framer → Decoder → LogSink
                                                                                  │
                                                                                  ▼
                                                                          session-dir/ on disk
```

Server is the **single source of truth** for ring buffers, time alignment,
and persistence. UI only renders. CLI talks the same WSS wire.

## 3. Build / test / lint / verify

The canonical task runner is **`just`**.

| Goal                       | Command                |
| -------------------------- | ---------------------- |
| Build everything           | `just build`           |
| Format check               | `just fmt-check`       |
| Lint (deny warnings)       | `just clippy`          |
| Tests                      | `just test`            |
| Driver-free local smoke    | `just local-smoke`     |
| Driver-free GUI smoke      | `just gui-smoke`       |
| Security audit             | `just audit`           |
| License/source policy      | `just deny`            |
| Coverage (llvm-cov)        | `just coverage`        |
| Benchmarks                 | `just bench`           |
| Fuzz smoke (60 s each)     | `just fuzz-smoke`      |
| Mutation tests             | `just mutants`         |
| RTM regeneration           | `just rtm`             |
| **Aggregate AI gate**      | **`just ai-verify`**   |

`just ai-verify` is the **gate every AI-authored PR must pass** before
requesting human review. It produces `target/ai-verify.json` consumed by
the server's `/api/ai/verify` endpoint.

Toolchain pin: `rust-toolchain.toml` → 1.88. Web: pnpm workspaces.

### Running the GUI (dev mode)

| Goal                           | Command                          |
| ------------------------------ | -------------------------------- |
| Backend server only            | `just dev-server`                |
| Web UI only (browser)          | `just dev-web`                   |
| Backend + Web UI together      | `just dev-all`                   |
| Tauri desktop (prep required)  | `just dev-prepare && just dev-tauri` |

**First-time Tauri setup** (Windows):

1. Add Defender exclusions (one-time, to unblock `build-script-build.exe`):
   ```pwsh
   Start-Process pwsh -Verb RunAs -Wait -ArgumentList '-Command',
     "Add-MpPreference -ExclusionPath '$env:CARGO_HOME','<target-dir>'; Add-MpPreference -ExclusionProcess '$env:CARGO_HOME\bin\cargo.exe'"
   ```
2. `pnpm install`
3. `just dev-prepare`   ← builds CLI sidecar + generates placeholder icons
4. `just dev-tauri`     ← starts Vite + Tauri window

`app-tauri/src-tauri/binaries/` and `app-tauri/src-tauri/icons/` are
generated (`.gitignore`d). Re-run `just dev-prepare` after rebuilding the
CLI binary. Production icons must be replaced before a release build.

## 4. Directory map

The map below is also exposed (machine-parseable) at the bottom of this file
under `<!-- map.toml -->`. Keep both in sync.

| Path                                | Role                                          | Stability     |
| ----------------------------------- | --------------------------------------------- | ------------- |
| `crates/core/src/source/`           | `Source` trait + impls (serial/tcp/udp/…)     | **stable**    |
| `crates/core/src/sink/`             | `Sink` trait + impls (write-back)             | **stable**    |
| `crates/core/src/framer/`           | `Framer` trait + impls                        | **stable**    |
| `crates/core/src/decoder/`          | `Decoder` trait + impls                       | **stable**    |
| `crates/core/src/logsink/`          | `LogSink` trait + impls                       | **stable**    |
| `crates/core/src/importer/`         | `Importer` trait + impls                      | stable        |
| `crates/core/src/exporter/`         | `Exporter` trait + impls                      | stable        |
| `crates/core/src/timeseries/`       | `TimeseriesSink` (parquet)                    | experimental  |
| `crates/core/src/time/`             | `TimeSource`, dual TS, NodeClockTable         | **stable**    |
| `crates/core/src/log/`              | session-dir on-disk format (raw/index/lines)  | **stable**    |
| `crates/core/src/session/`          | session registry + fan-out + ring buffer      | **stable**    |
| `crates/core/src/config/`           | TOML config + migration                       | **stable**    |
| `crates/core/src/detect/`           | auto-detect probes (serial/tcp/udp)           | stable        |
| `crates/core/src/codec.rs`          | EOL / encoding (encoding_rs)                  | **stable**    |
| `crates/core/src/secret.rs`         | `secret://` resolver (keyring)                | **stable**    |
| `crates/core/src/error_id.rs`       | E-NNNN registry                               | **stable**    |
| `crates/core/src/eventbus.rs`       | broadcast bus (drop-on-lag)                   | stable        |
| `crates/core/src/metrics.rs`        | Prometheus metrics (feature `metrics`)        | stable        |
| `crates/server/`                    | axum + rustls + WSS mux + ingest + ai_api     | **stable**    |
| `crates/cli/`                       | `tracemux` binary (clap)                     | stable        |
| `crates/replay/`                    | replay engine (offline session-dir → fan-out) | stable        |
| `crates/fuzz/`                      | cargo-fuzz targets                            | experimental  |
| `app-tauri/`                        | Tauri 2 shell (sidecar `serve` + WSS UI)      | stable        |
| `web/`                              | SolidJS UI, xterm.js + Dockview               | stable        |
| `templates/`                        | scaffolding templates for new sources/etc.    | stable        |
| `docs/protocols/`                   | wire-protocol / log-format / cli-output specs | **CRITICAL**  |
| `docs/adr/`                         | architecture decision records                 | **CRITICAL**  |
| `docs/requirements.md`              | FR-/NFR- requirements                         | **CRITICAL**  |
| `docs/rtm.md`                       | requirements traceability matrix (generated)  | generated     |
| `docs/errors/`                      | E-NNNN error catalogue                        | **stable**    |
| `.github/skills/`                   | task-specific AI skill manuals                | stable        |
| `fixtures/`                         | golden inputs for compat / fuzz / replay      | **stable**    |

## 5. Critical paths (require **human review**)

A PR is auto-labelled `human-review-required` (see
`.github/workflows/label-critical.yml`) and may not be merged by an AI
agent if it touches any of these paths:

- `docs/protocols/wire-protocol.md`
- `docs/protocols/log-format.md`
- `docs/protocols/timestamp.md`
- `docs/protocols/cli-output/**`
- `docs/adr/**`
- `docs/requirements.md`
- `docs/invariants.md`
- `crates/core/src/source/mod.rs` (trait)
- `crates/core/src/sink/mod.rs` (trait)
- `crates/core/src/framer/mod.rs` (trait)
- `crates/core/src/decoder/mod.rs` (trait)
- `crates/core/src/logsink/mod.rs` (trait)
- `crates/core/src/time/mod.rs` (trait + dual TS struct)
- `crates/core/src/log/wal.rs`
- `crates/core/src/log/group_commit.rs`
- `crates/core/src/log/rotate.rs`
- `crates/core/src/config/migrate.rs`
- `crates/core/src/secret.rs`
- `crates/server/src/wire.rs`
- `crates/server/src/auth.rs`
- `crates/server/src/tls.rs`
- `crates/server/src/fingerprint.rs`
- `Cargo.lock`
- `rust-toolchain.toml`
- `deny.toml`
- `.github/workflows/release.yml`
- `SECURITY.md`

The list is mirrored verbatim in `<!-- map.toml -->.critical_paths`.

## 6. Frozen v0.1 API surfaces

Three surfaces are independently semver-versioned and **must not** change
without an ADR + bumped version + a fixture-corpus compatibility test in CI:

1. **wire-protocol** (WSS subprotocol `tracemux.v1`, MessagePack frames).
   Spec: `docs/protocols/wire-protocol.md`. Compat: `tests/compat/wire/*`.
2. **log-format** (`session-dir/` layout). Spec:
   `docs/protocols/log-format.md`. Compat: `tests/compat/log/*` and
   `crates/replay/` consumes historical fixtures.
3. **cli-output** (`--format json` schemas under
   `docs/protocols/cli-output/v1/*.json`). Compat:
   `tests/compat/cli/*` snapshots.

The `app` (UI) version is independent and may evolve freely.

## 7. Pitfalls (read before coding)

- **Two timestamps are mandatory.** Every record carries
  `ts_origin` (best-known source time) **and** `ts_ingest`
  (server receive time). Plus `mono_ns`, `boot_id`, `node_id`,
  `clock_offset_ms`, `clock_quality`, `drift_ppm`, `clock_source`. Never
  drop one in favour of the other.
- **Backpressure is split.** Logger pipeline = `mpsc` bounded blocking
  (lossless). UI pipeline = `tokio::sync::broadcast` drop-on-lag. Don't
  cross the streams.
- **Server is the truth.** The browser/Tauri/CLI all talk WSS to the
  server. Never persist log data directly from the UI process.
- **Auth is non-optional except on loopback.** `--no-auth` is gated to
  `127.0.0.1`/`::1`. `argon2id` for tokens. TLS via `rustls` + TOFU
  fingerprint pin (`rcgen` self-signed cert).
- **Secrets never live in TOML.** Config stores `secret://name`; the
  actual secret lives in the OS keyring (`keyring` crate).
- **`unsafe_code = "deny"`.** Workspace-wide. Ask before relaxing.
- **Encodings via `encoding_rs`.** Don't roll your own Shift_JIS/EUC-JP.
- **Sources may be source-only.** pcap, RTT, CAN have no write path; they
  implement `Source` but not `Sink`. Don't conflate.
- **Binary protocols use Framer + Decoder.** Don't put bit-twiddling in
  a `Source`.
- **Every error has an E-NNNN id.** See `crates/core/src/error_id.rs` and
  `docs/errors/`.
- **All text files must be UTF-8 (no BOM) with LF line endings.**
  Some IDEs on Japanese Windows save files as Shift-JIS by default,
  which makes `cargo` choke (`stream did not contain valid UTF-8`).
  Run `pwsh scripts/check-encoding.ps1` to verify and
  `pwsh scripts/fix-encoding.ps1` to auto-convert. CI enforces this
  via the `encoding` job. `.editorconfig` and `.gitattributes` set the
  defaults.
- **Antivirus software (Windows Defender, AVG, etc.) may block
  `build-script-build.exe`.** A heuristic flags this exact filename
  (used by every cargo build script) as suspicious, surfacing as
  `os error 5 (access denied)` during `rustversion`/`rustls` builds.
  Confirmed on a machine where AVG Antivirus manages the Defender
  Firewall. Workaround: add **three** exceptions in your AV settings
  (General → Exceptions or equivalent):
  1. Folder exception for your cargo target dir (e.g. `%USERPROFILE%\rustcache2`)
  2. Folder exception for `%USERPROFILE%\.cargo`
  3. Process/app exception for `%USERPROFILE%\.cargo\bin\cargo.exe`
  Make sure all exceptions cover *all* shields (file shield + behaviour
  shield). The bug is local-machine only and never appears in CI.

## 8. AI workflow expectations

For non-trivial work, follow the matching skill in `.github/skills/`:

- Add a transport → `add-source/SKILL.md`
- Add a write-back → `add-sink/SKILL.md`
- Add a framer → `add-framer/SKILL.md`
- Add a decoder → `add-decoder/SKILL.md`
- Add an importer → `add-importer/SKILL.md`
- Add a UI panel → `add-ui-panel/SKILL.md`
- Investigate a logged issue → `investigate-log/SKILL.md`
- Reproduce a flaky channel → `repro-channel/SKILL.md`
- Tune performance → `perf-tune/SKILL.md`
- Cut a release → `release/SKILL.md`

Before you finish: **`just ai-verify`** must be green and
`docs/rtm.md` must be regenerated.

Open questions or ambiguous specs → propose an ADR under
`docs/adr/NNNN-<slug>.md` (use `docs/adr/template.md`) and reference it
from the PR.

## 9. Commit / PR conventions

- Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, …).
- PR template fields are mandatory: 受入条件 / 影響範囲 /
  wire・log・cli 互換性影響 / RTM 更新.
- Every requirement (`FR-…` / `NFR-…`) referenced by tests must appear
  in `docs/rtm.md`.
- `human-review-required` label cannot be removed by an AI.

---

<!-- map.toml
[map]
schema = "tracemux/agents-map/v1"

[[entries]]
path = "crates/core/src/source/mod.rs"
role = "trait:Source"
stability = "stable"
critical = true

[[entries]]
path = "crates/core/src/sink/mod.rs"
role = "trait:Sink"
stability = "stable"
critical = true

[[entries]]
path = "crates/core/src/framer/mod.rs"
role = "trait:Framer"
stability = "stable"
critical = true

[[entries]]
path = "crates/core/src/decoder/mod.rs"
role = "trait:Decoder"
stability = "stable"
critical = true

[[entries]]
path = "crates/core/src/logsink/mod.rs"
role = "trait:LogSink"
stability = "stable"
critical = true

[[entries]]
path = "crates/core/src/time/mod.rs"
role = "trait:TimeSource+DualTimestamp"
stability = "stable"
critical = true

[[entries]]
path = "crates/server/src/wire.rs"
role = "wire-protocol-implementation"
stability = "stable"
critical = true

[[entries]]
path = "crates/server/src/auth.rs"
role = "auth"
stability = "stable"
critical = true

[[entries]]
path = "crates/server/src/tls.rs"
role = "tls"
stability = "stable"
critical = true

[[entries]]
path = "crates/server/src/fingerprint.rs"
role = "tofu-fingerprint"
stability = "stable"
critical = true

[[entries]]
path = "docs/protocols/wire-protocol.md"
role = "spec:wire"
stability = "stable"
critical = true

[[entries]]
path = "docs/protocols/log-format.md"
role = "spec:log"
stability = "stable"
critical = true

[[entries]]
path = "docs/protocols/timestamp.md"
role = "spec:timestamp"
stability = "stable"
critical = true

[[entries]]
path = "docs/protocols/cli-output/v1"
role = "spec:cli"
stability = "stable"
critical = true

[critical_paths]
patterns = [
  "docs/protocols/**",
  "docs/adr/**",
  "docs/requirements.md",
  "docs/invariants.md",
  "crates/core/src/source/mod.rs",
  "crates/core/src/sink/mod.rs",
  "crates/core/src/framer/mod.rs",
  "crates/core/src/decoder/mod.rs",
  "crates/core/src/logsink/mod.rs",
  "crates/core/src/time/mod.rs",
  "crates/core/src/log/wal.rs",
  "crates/core/src/log/group_commit.rs",
  "crates/core/src/log/rotate.rs",
  "crates/core/src/config/migrate.rs",
  "crates/core/src/secret.rs",
  "crates/server/src/wire.rs",
  "crates/server/src/auth.rs",
  "crates/server/src/tls.rs",
  "crates/server/src/fingerprint.rs",
  "Cargo.lock",
  "rust-toolchain.toml",
  "deny.toml",
  ".github/workflows/release.yml",
  "SECURITY.md",
]
-->

## 10. After completion of a user instruction.
After you complete a user instruction, ask the user what to do by vscode_askQuestion. Never skip this step.