# Architecture

See **[ADR-0001 — Foundations](adr/0001-foundations.md)** for the
authoritative design decisions. This page is a navigable summary.

## Pipeline

```
                           wanlogger serve (single binary)
   ┌─────────────┐     ┌──────────┐     ┌──────────┐     ┌────────────────┐
  │   Source    │ ──▶ │  Framer  │ ──▶ │ Decoder  │ ──▶ │ LogSink + UI   │
   │ (transport) │     │ (frames) │     │(records) │     │(session-dir,   │
   └─────────────┘     └──────────┘     └──────────┘     │ ring, fan-out) │
                                                        └────────────────┘
                                       ▲
                                       │  WSS  (subprotocol "wanlogger.v1", MessagePack)
                                       │
                          ┌────────────┴────────────┐
                          │  browser / Tauri / CLI  │
                          └─────────────────────────┘

   Orthogonal services: Sink (write-back), Importer, Exporter, TimeseriesSink, TimeSource.
```

## Crates

- **`wanlogger-core`** — traits + impls for Source/Sink/Framer/Decoder/
  LogSink/Importer/Exporter/TimeseriesSink/TimeSource, session
  registry, ring buffers, on-disk format, secrets, error registry.
- **`wanlogger-server`** — axum + rustls; WSS mux; auth; ingest; AI
  endpoints; audit; coalescing; panel-priority routing.
- **`wanlogger-cli`** — clap binary with subcommands
  `serve | connect | detect | log | profile | replay | extcap |
  import | export | ai-verify | json-schema`.
- **`wanlogger-replay`** — drives a session-dir back through the same
  pipeline (deterministic with `--seed`).

## Apps

- **`app-tauri/`** — Tauri 2 shell that sidecars `wanlogger serve` on
  loopback and connects via WSS.
- **`web/`** — SolidJS + xterm.js (WebGL) + Dockview UI, deployable
  standalone (browser) or inside Tauri.

## Multi-PC time

See [protocols/timestamp.md](protocols/timestamp.md). Every record
carries dual timestamps + `clock_offset_ms`. The server maintains a
NodeClockTable persisted to `session-dir/clock-table.jsonl`.

## Performance shape

- Up to 1000 concurrent sources; tile virtualization (N=16 visible).
- Server coalesces per panel-priority: 16 ms / 500 ms / 2 s buckets.
- WebGL terminal renderer; CPU fallback documented.
- Logger pipeline never drops; UI pipeline `lagged()` is observable.
