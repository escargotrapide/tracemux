# Architecture

See **[ADR-0001 — Foundations](adr/0001-foundations.md)** for the
authoritative design decisions. This page is a navigable summary.

## Pipeline

```
                          tracemux serve (single binary)
   ┌─────────────┐     ┌──────────┐     ┌──────────┐     ┌────────────────┐
  │   Source    │ ──▶ │  Framer  │ ──▶ │ Decoder  │ ──▶ │ LogSink + UI   │
   │ (transport) │     │ (frames) │     │(records) │     │(session-dir,   │
   └─────────────┘     └──────────┘     └──────────┘     │ ring, fan-out) │
                                                        └────────────────┘
                                       ▲
                                       │  WSS  (subprotocol "tracemux.v1", MessagePack)
                                       │
                          ┌────────────┴────────────┐
                          │  browser / Tauri / CLI  │
                          └─────────────────────────┘

   Orthogonal services: Sink (write-back), Importer, Exporter, TimeseriesSink, TimeSource.
```

## Crates

- **`tracemux-core`** — traits + impls for Source/Sink/Framer/Decoder/
  LogSink/Importer/Exporter/TimeseriesSink/TimeSource, session
  registry, ring buffers, on-disk format, secrets, error registry.
- **`tracemux-server`** — axum + rustls; WSS mux; auth; ingest;
  source lifecycle manager; source runner; AI endpoints; audit;
  coalescing; panel-priority routing.
- **`tracemux-cli`** — clap binary with subcommands
  `serve | connect | detect | log | profile | replay | extcap |
  import | export | ai-verify | json-schema`.
- **`tracemux-replay`** — drives a session-dir back through the same
  pipeline (deterministic with `--seed`).

## Apps

- **`app-tauri/`** — Tauri 2 shell that sidecars `tracemux serve` on
  loopback and connects via WSS.
- **`web/`** — SolidJS + xterm.js (WebGL) + Dockview UI, deployable
  standalone (browser) or inside Tauri.

## Source lifecycle and UI sync

The WSS `ctl` frame owns source lifecycle operations. The browser sends
`action: start | stop | resume | restart | remove | list`; the server
executes those operations through `SourceManager` and returns lifecycle
events or a full `sources` snapshot. The browser requests `list` on
connect/reconnect and after lifecycle acknowledgements so the table
converges back to server truth.

`tracemux serve` can also seed source lifecycle state at process start
from a v1 TOML config file (`--config tracemux.toml`). The config file
can set server startup defaults such as bind address, session root,
encoding, content-detection mode, session naming pattern, auth policy,
TLS state, retention keep-days, serial startup, export defaults, live
WSS delivery pacing, token PHC file references, and named startup
channels. Command-line flags remain the final override for overlapping
scalar values, while token PHC files from config and CLI are combined.
Configured channel labels are stored as session labels so source
snapshots and the UI show the operator-facing name.

Terminal panels subscribe with `sub`/`unsub` to `(sid, ch)` fan-out
streams registered by ingest. A source row can select and focus the
global terminal target; the terminal never subscribes to a placeholder
SID.

## Multi-PC time

See [protocols/timestamp.md](protocols/timestamp.md). Every record
carries dual timestamps + `clock_offset_ms`. The server maintains a
NodeClockTable persisted to `session-dir/clock-table.jsonl`.

## Performance shape

- Up to 1000 concurrent sources; tile virtualization (N=16 visible).
- Server coalesces per panel-priority: 16 ms / 500 ms / 2 s buckets.
- WebGL terminal renderer; CPU fallback documented.
- Logger pipeline never drops; UI pipeline `lagged()` is observable.
- The web metrics panel includes local `ui.*` counters for received
  frames, source updates, subscription dispatches, active subscriptions,
  and bounded-toast drops.
