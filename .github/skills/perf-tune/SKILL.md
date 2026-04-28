---
name: perf-tune
description: Diagnose and improve throughput / latency without breaking compat
---

# Skill: performance tuning

## Steps

1. Reproduce numbers with `just bench`. Compare to
   `bench-baseline.json` (CI fails on >10% regression).
2. Profile:
   - CPU: `cargo flamegraph -p wanlogger-server`.
   - Allocs: `dhat-rs` or `heaptrack`.
   - Async: `tokio-console` (server already integrates it under the
     `--debug-tokio` flag).
3. UI: Chrome DevTools Performance + xterm WebGL frame stats.
4. Identify which layer is hot (Source / Framer / Decoder / LogSink /
   transport). Focus on **one layer per PR** and keep API surfaces
   unchanged.
5. Add or update Criterion bench under `benches/<area>.rs`. Save new
   baseline only after human approval (it gates CI).
6. Document the change in an ADR if it alters the perf model
   (e.g. coalescing windows, ring sizes, fan-out fan-in).

## Pitfalls

- Do **not** lower fsync frequency on the WAL to chase numbers; that
  breaks the lossless logger contract.
- Do not relax `unsafe_code = "deny"` to use unchecked indexing;
  prefer iterator/slice patterns.
