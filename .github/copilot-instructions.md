# Copilot instructions for wanlogger

**Read [AGENTS.md](../AGENTS.md) first.** It is the canonical source for
architecture, critical paths, build commands, and pitfalls. This file is
the short, Copilot-optimised summary.

## TL;DR

- Rust workspace + pnpm web. Toolchain pinned in `rust-toolchain.toml`.
- Four-layer pipeline: **Source → Framer → Decoder → LogSink/UI**.
  Plus orthogonal `Sink` (write-back), `Importer`, `Exporter`,
  `TimeseriesSink`, `TimeSource`. All traits are **frozen v0.1**.
- Server is the single source of truth. Browser/Tauri/CLI talk WSS
  (`wanlogger.v1` MessagePack). Never persist from the UI.
- Every record has **dual timestamps**: `ts_origin` + `ts_ingest` plus
  `mono_ns`, `boot_id`, `node_id`, `clock_offset_ms`, `clock_quality`.
- `unsafe_code = "deny"` workspace-wide. `cargo deny` bans `openssl-sys`.
- Secrets live in OS keyring; config refers to them via `secret://name`.
- Every error has an `E-NNNN` id (see `crates/core/src/error_id.rs`).

## Before opening a PR

1. Pick the matching skill in `.github/skills/<task>/SKILL.md` and
   follow it.
2. Run `just ai-verify` until green. It produces
   `target/ai-verify.json`.
3. Regenerate `docs/rtm.md` (`just rtm`).
4. Fill the PR template (受入条件 / 影響範囲 / 互換性 / RTM).
5. If you touched any path listed under "Critical paths" in
   `AGENTS.md`, expect the `human-review-required` label and **do not
   self-merge**.

## Style

- Conventional Commits.
- `cargo fmt` + `clippy -D warnings` + 100-col lines.
- Public items in `crates/core` need rustdoc.
- Tests live next to code (`#[cfg(test)] mod tests`); integration tests
  under `crates/<crate>/tests/`. Compat fixtures under `tests/compat/`.
- Web: SolidJS + xterm.js + Dockview. State in
  `web/src/state/`. i18n keys under `web/src/i18n/`.

## When you are stuck

- Lookups: search `docs/adr/`, `docs/protocols/`, `docs/errors/`.
- Ambiguous spec → propose an ADR (`docs/adr/template.md`).
- Cross-cutting refactor → open a discussion issue first; do not change
  trait surfaces without ADR + version bump.
