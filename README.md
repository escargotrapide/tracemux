# wanlogger

> _That one unified terminal to view and maintain all the logs, either
> local or over networks._

A lightweight, high-functionality, multi-connection debug terminal and
log platform. Single Rust binary that runs as **CLI**, **server**, and
**Tauri sidecar**, plus a SolidJS web UI. Designed end-to-end for
AI-driven development with strong human-review guardrails on critical
paths.

## Status

**v0.1 ? executable vertical slice in progress.** Trait surfaces are
frozen. The server can now start selected sources, route WSS
subscriptions, persist source output to a session-dir, and drive the
SolidJS source/terminal UI. Some source kinds and release-hardening
features remain stubbed or experimental; see
[`docs/structure.md`](docs/structure.md) for what is filled in.

## Highlights

- **Four-layer pipeline**: `Source Å® Framer Å® Decoder Å® LogSink/UI`
  plus orthogonal `Sink`, `Importer`, `Exporter`, `TimeseriesSink`,
  `TimeSource`. All trait surfaces are frozen at v0.1.
- **Server is the source of truth.** Browser, Tauri shell, and CLI all
  speak the same WSS wire protocol (`wanlogger.v1`, MessagePack).
- **Dual timestamps** on every record (`ts_origin` + `ts_ingest` plus
  `mono_ns`, `boot_id`, `node_id`, `clock_offset_ms`,
  `clock_quality`) ? multi-PC log alignment is a first-class concern.
- **Secure-by-default**: rustls + `argon2id` bearer tokens + TOFU
  fingerprint pin + OS keyring secrets + `unsafe_code = "deny"`.
- **AI-maintainable**: `AGENTS.md`, `.github/skills/<task>/SKILL.md`,
  ADR + RTM, `human-review-required` label gate, `just ai-verify`.
- **Independent semver** for `wire-protocol`, `log-format`,
  `cli-output`, and `app`.

## Read this first

1. **[AGENTS.md](AGENTS.md)** ? canonical map (build, layout,
   critical paths, pitfalls).
2. **[docs/architecture.md](docs/architecture.md)** ? pipeline diagram.
3. **[docs/adr/0001-foundations.md](docs/adr/0001-foundations.md)** ?
   the foundational decisions.
4. **[SECURITY.md](SECURITY.md)** ? threat model and defaults.

## Quickstart (once toolchains are installed)

```bash
# Install Rust per rust-toolchain.toml and just
cargo install just
just build           # build the workspace
just test            # run tests
just ai-verify       # full gate (fmt + clippy + test + audit + deny + Åc)
```

For local UI development:

```bash
just dev-server      # loopback server at 127.0.0.1:9000, --no-auth
just dev-web         # SolidJS UI pointing at the loopback server
just dev-prepare     # build/copy the Tauri sidecar before desktop dev
just dev-tauri       # Tauri shell with bundled loopback sidecar
```

The web UI can start sources from URI-style specs such as
`mock://demo`, `file:///C:/logs/app.log?follow=1`, or
`tcp://127.0.0.1:5555`. Source presets are browser-local and store only
the source spec, never log data.

## License

Dual-licensed under MIT or Apache-2.0 at your option. See `LICENSE`.

