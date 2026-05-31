# ADR-0001: Foundations

- **Status:** Accepted
- **Date:** 2025-01-01
- **Deciders:** founders
- **Related requirements:** FR-CORE-*, NFR-*
- **Affected critical paths:** all

## Context

`tracemux` must be a single, lightweight binary that can replace and
go beyond Tera Term, RealTerm, and assorted log tailers, while:

- supporting many heterogeneous transports (serial, TCP, UDP, file,
  pipe, process, MQTT, syslog, HTTP webhooks, journald, Windows Event
  Log, ETW, J-Link RTT, CAN, …) with room to add Telnet/SSH/VISA later;
- aligning timestamps across multiple PCs / nodes;
- being driven by AI agents end-to-end (spec → code → verify → release)
  with strong guardrails;
- being secure on the public Internet (TLS, auth, no plaintext
  secrets);
- being viewable from a browser, a Tauri shell, **and** a CLI talking
  the same wire protocol;
- staying lightweight at 100-1000 simultaneous sources.

## Decision

We adopt the following foundational choices. They are **frozen at v0.1**
and may only be amended by a superseding ADR plus version bumps.

### 1. Four-layer pipeline + orthogonal services

```
Source → Framer → Decoder → LogSink / UI
```

Plus orthogonal: `Sink` (write-back), `Importer`, `Exporter`,
`TimeseriesSink`, `TimeSource`. Each layer is a Rust trait in
`crates/core/src/<layer>/mod.rs`.

`Source` and `Sink` are split because some transports (pcap, RTT, CAN
sniff) are read-only.

### 2. Server is the single source of truth

The Tauri shell launches a sidecar `tracemux serve` on
`127.0.0.1:auto` and connects via WSS. The browser and the CLI talk the
same wire. UI never persists log data.

### 3. Wire protocol = WSS subprotocol `tracemux.v1`, MessagePack

Frame: `{ type, sid?, ch?, seq, payload }`. Types:
`hello / auth / sub / unsub / data / ctl / write / metrics /
clientlog / ping / pong / clock_sync / panel_priority / child(reserved)`.
Auth via `Sec-WebSocket-Protocol: tracemux.v1, bearer.<token>`.
`permessage-deflate` text-only. Per-connection mux. Future
WebTransport(HTTP/3) capable. Spec: `docs/protocols/wire-protocol.md`.

### 4. Dual timestamps, mandatory

Every record carries `ts_origin` (best-known source time) **and**
`ts_ingest` (server receive time). Plus `mono_ns`, `boot_id`, `node_id`,
`clock_offset_ms`, `clock_quality`, `drift_ppm`, `clock_source`.
Clock sync: WSS `clock_sync` ping/pong every 30 s →
`session-dir/clock-table.jsonl`. Spec: `docs/protocols/timestamp.md`.

### 5. Backpressure separation

- Logger pipeline: `mpsc` bounded blocking → group-commit fsync WAL.
  **Lossless.**
- UI pipeline: `tokio::sync::broadcast` drop-on-lag.
- High-rate sources zero-copy fan-out via `Bytes::clone`.
- Server `RingBuffer` per connection (default 8 MiB).

### 6. Independent semver per surface

- `wire-protocol` — `docs/protocols/wire-protocol.md`
- `log-format`    — `docs/protocols/log-format.md`
- `cli-output`    — `docs/protocols/cli-output/v1/*.json`
- `app`           — UI / Tauri (free to evolve)

CI keeps a fixture corpus per surface; bumps require an ADR.

### 7. Security defaults

- `argon2id` (m=64 MB, t=3, p=1) for tokens.
- OS keyring (`keyring` crate) holds secrets; config refers via
  `secret://name`.
- TLS via `rustls` + `rcgen` self-signed; TOFU fingerprint pin.
- `--no-auth` gated to loopback (`127.0.0.1`, `::1`).
- WSS DoS limits: at most 32 connections, at most 1 MiB per frame, 1 KiB/s rate.
- Files 0600 / Windows ACL Owner-only.
- `unsafe_code = "deny"` workspace-wide; `cargo deny` bans
  `openssl-sys`.
- `cargo audit` + `cargo deny` + `pnpm audit` in CI.
- Releases cosign-signed; no auto-update.

### 8. AI-driven workflow

- Every requirement has an id (`FR-` / `NFR-`); tests reference them.
- `docs/rtm.md` is generated.
- Critical paths gated by `human-review-required` label.
- Skills under `.github/skills/<task>/SKILL.md` capture playbooks.
- `just ai-verify` is the gate (fmt/clippy/test/audit/deny/coverage/
  bench/fuzz_smoke/rtm + JSON summary at `target/ai-verify.json`).
- Server exposes `/api/ai/verify` for self-checks.

## Consequences

- **Positive:** clean separation, AI agents can extend without
  destabilising trait surfaces; multi-PC time alignment is a first-
  class concern; strong security defaults.
- **Negative:** more boilerplate at v0.1 (every transport must implement
  Source + optionally Sink and possibly route through Framer/Decoder);
  upfront cost in compat fixtures.
- **Compatibility impact:** all four versioned surfaces start at v1.

## Alternatives considered

1. *Single monolithic transport trait* — rejected: forces source-only
   transports to implement degenerate write paths; couples framing to
   transport.
2. *gRPC over HTTP/2* — rejected for v0.1: harder to debug from a
   browser, no native browser client, larger dep surface than WSS +
   MessagePack. Re-considered if WebTransport(HTTP/3) lands.
3. *Single timestamp* — rejected: cross-PC analysis breaks when a node
   has bad clock; dual TS lets us recover.
4. *Loss-tolerant logger* — rejected: contradicts "maintain logs"
   product mission.

## Migration plan

N/A — this is the initial decision.
