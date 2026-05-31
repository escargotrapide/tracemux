---
name: add-sink
description: Add a write-back sink (e.g. send bytes back to a Source)
---

# Skill: add a new Sink

A `Sink` accepts bytes / control to write back to a connected channel
(serial TX, TCP send, MQTT publish, …). Source-only transports
(pcap/RTT/CAN) **must not** have one.

## Steps

1. Read [`crates/core/src/sink/mod.rs`](../../../crates/core/src/sink/mod.rs).
   Frozen v0.1.
2. Implement `Sink` in `crates/core/src/sink/<name>.rs`. Wire it into
   `sink/mod.rs`.
3. If the matching `Source` is in this repo, register the pairing in
   `crates/core/src/session/registry.rs`.
4. Add tests in `crates/core/tests/sink_<name>.rs` exercising
   ordering, backpressure, and error mapping.
5. Add `FR-SINK-<name>` to `docs/requirements.md`.
6. `just ai-verify`. PR with standard template.

## Pitfalls

- Sinks must respect the wire-protocol `write` frame's `seq` ordering.
- Don't bypass the server's audit log: writes through the WSS path are
  audited in `crates/server/src/audit.rs`.
