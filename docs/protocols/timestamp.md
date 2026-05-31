# Timestamp & clock model

> **Frozen v0.1.**

## Why dual timestamps?

`tracemux` is often deployed across multiple PCs or devices whose
clocks may disagree by milliseconds to minutes. A single timestamp
forces a tradeoff:

- *source-time only* breaks when a node's clock is wrong.
- *server-time only* breaks when network jitter or buffering is large.

So **every record carries both**.

## Fields

| Field             | Meaning                                                |
| ----------------- | ------------------------------------------------------ |
| `ts_origin`       | Best-known time at the *source* (parsed from frame, or producer-side wallclock). |
| `ts_ingest`       | Wallclock time at the *server* when the record was received. |
| `mono_ns`         | Server-side monotonic ns (immune to wallclock jumps).  |
| `boot_id`         | UUID per server boot; resets when monotonic does.      |
| `node_id`         | UUID per producing node.                               |
| `clock_offset_ms` | Estimated `node.wallclock — server.wallclock` (ms).    |
| `clock_quality`   | `synced` (NTP/PTP), `best-effort`, `unknown`, `imported`. |
| `drift_ppm`       | Estimated drift between node and server (ppm).         |
| `clock_source`    | `system`, `ntp`, `ptp`, `monotonic`, `imported`.       |

## Clock sync exchange

- WSS frame `clock_sync` is exchanged every 30 s by default.
- Round-trip is computed Cristian-style:
  `offset = ((t2 - t1) + (t3 - t4)) / 2`.
- Results are appended to `session-dir/clock-table.jsonl`:

```jsonc
{ "ts": "...", "node_id": "...", "rtt_ms": 5, "offset_ms": -2, "drift_ppm": 1.2, "quality": "synced" }
```

## Display rules

- UI sorts by `ts_origin` by default; users can switch to `ts_ingest`.
- The Correlation panel uses `ts_origin + clock_offset_ms`.
- When `clock_quality == "unknown"`, UI shows a yellow badge and falls
  back to `ts_ingest`.

## Imported logs

Importers set `ts_origin` from the source artefact, `ts_ingest = now()`
at import time, and `clock_quality = "imported"`. Replay tooling must
preserve these as-is.
