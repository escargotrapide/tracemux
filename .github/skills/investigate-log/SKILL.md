---
name: investigate-log
description: Investigate a logged issue / triage from a session-dir
---

# Skill: investigate a logged issue

When the user shares a `session-dir/` (or a piece of one), follow this
protocol.

## Steps

1. `meta.toml` first: confirm app/version, source/framer/decoder, host.
2. `index.jsonl` tail: find the latest `level >= warn` entries. Match
   `correlation_id` across sessions if multi-PC.
3. Look up every `E-NNNN` reference in `docs/errors/E-NNNN.md`.
4. If decoder failed: replay against the same `schema_id` from
   `schemas/<id>.json`. Mismatches imply a `add-decoder` follow-up.
5. Reproduce locally:
   `cargo run -p tracemux-cli -- replay --session <dir> --filter ...`.
6. If timing looks wrong cross-PC, inspect `clock-table.jsonl` and
   `clock_offset_ms` per record. See `docs/protocols/timestamp.md`.
7. File a `bug.yml` issue with: minimal repro, `E-NNNN`, expected vs
   actual, `clock_quality` if relevant.

## Pitfalls

- Don't trust `ts_origin` alone — always cross-check `ts_ingest` and
  `clock_quality`.
- `raw.bin` is zstd-framed; use the replay tool, not raw `cat`.
