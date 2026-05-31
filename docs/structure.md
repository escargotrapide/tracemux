# Project structure

```
.
├── AGENTS.md                         # canonical AI/contributor map
├── Cargo.toml                        # Rust workspace
├── rust-toolchain.toml               # pinned toolchain
├── justfile                          # task runner (just <task>)
├── deny.toml                         # cargo-deny policy
├── pnpm-workspace.yaml               # web workspaces
├── cliff.toml                        # changelog generator config
├── SECURITY.md                       # threat model, defaults
├── CONTRIBUTING.md
├── CHANGELOG.md
├── crates/
│   ├── core/                         # frozen v0.1 traits + impls
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── source/   sink/   framer/   decoder/
│   │       ├── logsink/  importer/   exporter/   timeseries/
│   │       ├── time/                 # dual-timestamp model
│   │       ├── log/                  # session-dir on disk (WAL etc.)
│   │       ├── session/              # registry + ring + fanout
│   │       ├── config/   detect/
│   │       ├── codec.rs  secret.rs   error_id.rs
│   │       └── eventbus.rs   metrics.rs
│   ├── server/                       # axum + rustls + WSS mux + source lifecycle
│   ├── cli/                          # `tracemux` binary
│   └── replay/                       # session-dir replay engine
├── docs/
│   ├── architecture.md
│   ├── invariants.md
│   ├── requirements.md
│   ├── rtm.md                        # generated
│   ├── adr/{template, 0001-foundations}.md
│   ├── protocols/{wire-protocol, log-format, timestamp}.md
│   ├── protocols/cli-output/v1/      # JSON schemas (snapshotted)
│   └── errors/README.md              # E-NNNN catalogue
├── templates/source/...               # scaffolds
├── scripts/{gen-rtm, ai-verify-summary, release-gate}.sh
└── .github/
    ├── copilot-instructions.md
    ├── ISSUE_TEMPLATE/{feature, bug, task}.yml
    ├── PULL_REQUEST_TEMPLATE.md
    ├── skills/{add-source, add-sink, add-framer, add-decoder,
    │           add-importer, add-ui-panel, investigate-log,
    │           repro-channel, perf-tune, release}/SKILL.md
    └── workflows/{ci, label-critical}.yml
```

Recently-filled executable slices:

- `crates/server/src/runner.rs` — Source -> Framer -> Decoder ->
  LogSink/UI fan-out vertical slice.
- `crates/server/src/source_manager.rs` — start/stop/resume/restart/
  remove/list lifecycle manager with optional session-dir persistence.
- `crates/core/src/logsink/file.rs` — append-only `FileLogSink` for
  `meta.toml`, `raw.bin`, `index.jsonl`, `lines.jsonl`, and
  `frames.jsonl`.
- `web/src/state/sourceSpec.ts` — URI-style source spec parser for the
  source panel.
- `web/src/state/sourcePresets.ts` — browser-local source preset/profile
  storage (specs only; no log data).
- `web/src/state/sourceFilters.ts` — source search/filter/sort helper.

Where things are **not yet** filled in (deferred to next iterations):

- `crates/fuzz/`        — cargo-fuzz targets.
- `benches/` + `bench-baseline.json` — Criterion benches.
- `docs/errors/E-*.md`  — one file per allocated id.
