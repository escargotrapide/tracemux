# PR summary: `development` → `main` (v0.1.0 release-readiness)

> Generated overview of the delta between `origin/main` and
> `origin/development`. Use this to fill the PR template
> (受入条件 / 影響範囲 / 互換性 / RTM).

## At a glance

- **Commits:** 11 (`f9f11cf`..`90242c8`, plus earlier `aaeaba0`, `5528eb5`,
  `96c8533`)
- **Files changed:** 102
- **Lines:** +5563 / -421
- **Net theme:** the complete v0.1.0 release-readiness pass — version bump to
  `0.1.0`, UX/accessibility hardening, onboarding docs, packet-capture MVP
  polish, examples, fuzz scaffolding, and the deferred ADR-0003.

## Commits (newest first)

| Commit | Subject |
| ------ | ------- |
| `90242c8` | docs(changelog): polish v0.1.0 release notes |
| `96ec602` | chore(release): bump workspace version to 0.1.0 |
| `cb8a241` | docs: correct v0.1.0 deferred-roadmap accuracy |
| `0dceaaa` | docs(adr): propose ADR-0003 live snaplen-truncation signalling |
| `58463e2` | docs: refresh v0.1.0 known-limitations E2E coverage note |
| `344e60c` | feat(ui,cli): address v0.1.0 should-fix items |
| `8368c32` | feat(ui): finish v0.1.0 packet-capture and onboarding must-fix items |
| `f9f11cf` | feat(web): harden v0.1.0 terminal, tiles, and settings UX |
| `96c8533` | feat(web): let users pick which channel a source opens |
| `5528eb5` | feat(v0.1.0): audit hardening, sid-tagged source errors, local smoke |
| `aaeaba0` | feat: v0.1.0 UX stabilization pass |

## Changes by area

### Release / versioning

- Bumped workspace and all surface versions `0.1.0-dev` → `0.1.0`
  (`Cargo.toml`, crate path-dep pins, `Cargo.lock`, `app-tauri`,
  `web/package.json`, web WSS hello, `tools/virt-peer`).
- `CHANGELOG.md`: populated and polished the `[0.1.0]` section.

### Web UI (SolidJS)

- **Metrics panel:** unit-aware formatting (`formatMetric.ts`) with IEC byte
  units, durations, rates, ratios; a legend; scoped table headers.
- **Sources panel:** per-channel open selection, per-port bulk-open summaries,
  bulk-export with AbortController cancellation, progress labels, `aria-busy`
  on long-running actions, safer destructive actions.
- **Notification center:** focus-trapped modal dialog, Escape-to-close, focus
  return to trigger.
- **Terminal / Tiles / Settings:** connection-state feedback, source-start
  pending feedback, visible focus rings, display-settings and export-settings
  state.
- **i18n:** new keys added to **both** `en.json` and `ja.json` (parity enforced
  by `i18n.test.ts`).

### CLI

- `long_about` help text on every subcommand; clearer not-yet-implemented
  import-kind hints; mojibake fix in `detect.rs`.

### Core / server (Rust)

- Packet-capture MVP polish (`source/pcap.rs`), sid-tagged source errors,
  `logsink/file.rs` additions, new error ids (`error_id.rs`), benches
  (`framer_line.rs`, `packet_summary.rs`), example-config tests.
- `server/src/ws.rs` hardening; `tls.rs` adjustments.
- `crates/fuzz/` scaffolding (6 targets: decoder, framer, index_jsonl,
  telnet_iac, vt_escape, wire_proto).

### Docs

- New onboarding guides under `docs/guides/` (getting-started-windows,
  first-session, auth-and-tls, source-specs, performance-capacity).
- New error pages (`E-1103..E-1106`, `E-4001`, `E-4002`).
- New `examples/` configs (mock, serial, tcp-listener, multi-source,
  packet-capture).
- `docs/dev/v0.1.0-change-list.md` maintainer tracker; refreshed packet-capture
  dev docs; regenerated `docs/rtm.md`.

### Tests

- Web: many new unit tests (`formatMetric`, `displaySettings`, `errorRunbooks`,
  `sessionExportZip`, `sourceStartOptions`, `state`, `i18n`) and a large
  `shell.spec.ts` E2E expansion plus a real-backend E2E harness.

## ⚠️ Critical-path files (require human review — not AI self-merge)

These touched files match the `human-review-required` patterns in `AGENTS.md`:

- `Cargo.lock` — version bump regeneration.
- `crates/core/src/config/migrate.rs` — config migration.
- `crates/server/src/tls.rs` — TLS.
- `deny.toml` — license/source policy.
- `docs/adr/0003-live-snaplen-truncation-signalling.md` — ADR (Proposed).
- `docs/requirements.md` — requirements.

**Frozen surfaces (wire / log-format / cli-output) are NOT changed in this PR.**
ADR-0003 only *proposes* an additive `orig_len` field; it does not modify
`wire.rs` or `wire-protocol.md`.

## Compatibility (wire / log / cli)

- **wire-protocol `tracemux.v1`:** unchanged.
- **log-format (session-dir):** unchanged.
- **cli-output v1 schemas:** unchanged.
- **app (UI) version:** evolved (independent semver).

## Verification

- `just ai-verify`: green (encoding / fmt / clippy / test / rtm).
- `just release-gate`: `cargo audit` + `cargo deny` pass; only the
  expected pre-tag blocker (signed tag `v0.1.0` not yet created) remains.
- Encoding: all text files UTF-8 (no BOM), LF.

## RTM

- `docs/rtm.md` regenerated and committed (`just rtm`).

## Post-merge (human-owned)

- Create signed tag `v0.1.0` (`git tag -s v0.1.0`) and push → triggers
  `release.yml` (build + cosign-sign + publish).
- Verify on a clean machine: `cosign verify-blob` → `tracemux --version` →
  `tracemux ai-verify --self-test`.
