# Performance and capacity

TraceMux separates lossless persistence from best-effort UI delivery.

## Current targets

- The logger path is lossless within configured queue depth.
- The UI pipeline may drop for lagging subscribers, but drops should surface
  through counters or lag notifications.
- The UI target is more than 30 fps with 1000 sources at 1 KiB/s aggregate,
  using server coalescing and 16 visible tiles.
- Packet capture separates persistence from UI fan-out. High-rate captures
  should use BPF filters, snaplen, and `publish=stats-only` or `sampled`.

## Operator guidance

- Keep packet capture `publish=stats-only` until you need packet rows in the UI.
- Use BPF filters close to the capture source.
- Lower snaplen when payload bytes are not required.
- Prefer persisted export for large review tasks instead of keeping huge
  terminal buffers in the browser.
- Watch the Metrics panel for UI counters, toast drops, packet drops, and
  per-source rates.

## Verification commands

```powershell
just ai-verify
just ai-verify-full
```

Initial Criterion benches exist for core line framing and packet summaries.
Save or update baselines only after human approval because baselines become a
release gate:

```powershell
just bench
just bench-baseline
just fuzz-smoke
```

Initial cargo-fuzz targets also exist for wire, framer, decoder, index JSONL,
and early terminal-protocol byte boundaries. Planned hardening work still
includes high-volume UI E2E, reconnect recovery E2E, and packet-capture soak
checks.