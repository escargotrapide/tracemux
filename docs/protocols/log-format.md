# Log format — session-dir layout

> **Frozen v0.1.** Changes require ADR + bumped log-format version +
> fixture compat under `tests/compat/log/`.

## Directory naming

```
{prefix}_{kind}_{iface}_{YYYYMMDD-HHMMSS}/
```

- `prefix`  — user-supplied or `wanlogger` by default.
- `kind`    — `serial`, `tcp`, `udp`, `file`, `mqtt`, `mixed`, …
- `iface`   — `COM3`, `eth0`, `mosquitto-1`, …
- timestamp is the local `ts_ingest` of the first record.

## Files

| File                  | Description                                         |
| --------------------- | --------------------------------------------------- |
| `meta.toml`           | source / framer / decoder / codec spec, host, tags, app version |
| `raw.bin`             | raw bytes concatenated, **zstd-framed + WAL** (lossless) |
| `index.jsonl`         | per-record envelope (see schema below)              |
| `lines.jsonl`         | decoded text lines                                  |
| `frames.jsonl`        | structured records with `schema_id`                 |
| `schemas/<id>.json`   | JSON schemas referenced by `schema_id`              |
| `timeseries.parquet`  | optional numeric series                             |
| `clock-table.jsonl`   | 30 s NodeClockTable history (multi-PC sync)         |

## `index.jsonl` schema

```jsonc
{
  "ts_origin":       "RFC3339 with ns",
  "ts_ingest":       "RFC3339 with ns",
  "mono_ns":         123,
  "boot_id":         "uuid",
  "node_id":         "uuid",
  "clock_offset_ms": 0,
  "clock_quality":   "synced|best-effort|unknown|imported",
  "drift_ppm":       0.0,
  "clock_source":    "system|ntp|ptp|monotonic|imported",
  "sid":             "uuid",
  "dir":             "in|out",
  "kind":            "bytes|datagram|frame|record",
  "off":             0,             // offset into raw.bin
  "len":             0,             // length in raw.bin
  "level":           "info",        // optional
  "tags":            ["..."],       // optional
  "correlation_id":  "...",         // optional
  "source":          "serial:COM3", // optional
  "host":            "node-1",      // optional
  "schema_id":       "..."          // optional
}
```

## Rotation & retention

- Configured per-session in `meta.toml`:
  `rotate.size_mb`, `rotate.duration_min`, `retention.keep_days`.
- Rotation closes the current session-dir and opens a new one with
  the same prefix and a new timestamp suffix.

## WAL & group commit

- `raw.bin` writes go through a write-ahead log. fsync is performed
  every `commit_window_ms` (default 50 ms) or `commit_size_kib`
  (default 256 KiB), whichever first.
- Crash recovery replays the WAL on next session open.

## Versioning

| Surface     | Version | Source of truth   |
| ----------- | ------- | ----------------- |
| Layout      | `1.0.0` | this file         |
| Index row   | `1.0.0` | this file         |
| Compat      | fixtures| `tests/compat/log/v1/*` |
