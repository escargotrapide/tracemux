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
│   ├── server/                       # axum + rustls + WSS mux
│   ├── cli/                          # `wanlogger` binary
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

Where things are **not yet** filled in (deferred to next iterations):

- `crates/fuzz/`        — cargo-fuzz targets.
- `benches/` + `bench-baseline.json` — Criterion benches.
- `docs/protocols/cli-output/v1/*.schema.json` — emitted by
  `wanlogger json-schema`.
- `docs/errors/E-*.md`  — one file per allocated id.
