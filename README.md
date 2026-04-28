# wanlogger

> _That one unified terminal to view and maintain all the logs, either
> local or over networks._

A lightweight, high-functionality, multi-connection debug terminal and
log platform. Single Rust binary that runs as **CLI**, **server**, and
**Tauri sidecar**, plus a SolidJS web UI. Designed end-to-end for
AI-driven development with strong human-review guardrails on critical
paths.

## Status

**v0.1 — Phase 0 scaffolding complete.** Trait surfaces are frozen.
Most implementations are stubs that compile but `todo!()` at runtime.
See [`docs/structure.md`](docs/structure.md) for what's filled in.

## Highlights

- **Four-layer pipeline**: `Source → Framer → Decoder → LogSink/UI`
  plus orthogonal `Sink`, `Importer`, `Exporter`, `TimeseriesSink`,
  `TimeSource`. All trait surfaces are frozen at v0.1.
- **Server is the source of truth.** Browser, Tauri shell, and CLI all
  speak the same WSS wire protocol (`wanlogger.v1`, MessagePack).
- **Dual timestamps** on every record (`ts_origin` + `ts_ingest` plus
  `mono_ns`, `boot_id`, `node_id`, `clock_offset_ms`,
  `clock_quality`) — multi-PC log alignment is a first-class concern.
- **Secure-by-default**: rustls + `argon2id` bearer tokens + TOFU
  fingerprint pin + OS keyring secrets + `unsafe_code = "deny"`.
- **AI-maintainable**: `AGENTS.md`, `.github/skills/<task>/SKILL.md`,
  ADR + RTM, `human-review-required` label gate, `just ai-verify`.
- **Independent semver** for `wire-protocol`, `log-format`,
  `cli-output`, and `app`.

## Read this first

1. **[AGENTS.md](AGENTS.md)** — canonical map (build, layout,
   critical paths, pitfalls).
2. **[docs/architecture.md](docs/architecture.md)** — pipeline diagram.
3. **[docs/adr/0001-foundations.md](docs/adr/0001-foundations.md)** —
   the foundational decisions.
4. **[SECURITY.md](SECURITY.md)** — threat model and defaults.

## Quickstart (once toolchains are installed)

```bash
# Install Rust per rust-toolchain.toml and just
cargo install just
just build           # build the workspace
just test            # run tests
just ai-verify       # full gate (fmt + clippy + test + audit + deny + …)
```

## License

Dual-licensed under MIT or Apache-2.0 at your option. See `LICENSE`.

