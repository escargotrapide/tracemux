# ADR-0003: Live snaplen-truncation signalling for packet capture

- **Status:** Proposed
- **Date:** 2026-05-21
- **Deciders:** tracemux maintainers
- **Related requirements:** FR-SRC-PCAP, FR-UI-PCAP, FR-WIRE-001
- **Affected critical paths:** yes — `docs/adr/**`; if accepted, also
  `docs/protocols/wire-protocol.md`, `docs/protocols/log-format.md`,
  `crates/server/src/wire.rs`, and `crates/server/src/pcap_runner.rs`

## Context

The packet capture source applies a `snaplen` cap so each captured packet is
truncated to at most `snaplen` bytes (see `FR-SRC-PCAP` in
[docs/requirements.md](../requirements.md) and the libpcap default
`snaplen 65535`). When a packet on the wire is larger than `snaplen`, only the
first `snaplen` bytes are captured. Operators need to *see* that truncation
happened so they do not mistake a clipped packet for a short packet — otherwise
length-based analysis and BPF tuning are misleading.

The on-disk log already records both lengths. `pcap_runner.rs` writes a
`datagram` frame whose `frames.jsonl` record carries `captured_len` and
`original_len` fields (`crates/server/src/pcap_runner.rs`, the `append_packet`
path around the `"captured_len"` / `"original_len"` field inserts). The CLI/UI
export path and the web frontend already render `capturedLen`/`originalLen`, so
offline/replayed views can show truncation today.

The **live** path cannot. The live UI datagram is published via
`encode_data_envelope_with_kind(sid, 0, raw_frames, &ts, &packet.data, …,
"datagram")` in `crates/server/src/pcap_runner.rs`. That envelope conforms to
the frozen v0.1 wire `data` payload
([docs/protocols/wire-protocol.md](../protocols/wire-protocol.md)), whose
fields are:

```text
kind: "bytes" | "datagram" | "frame" | "record",
body: bin | map,
… (timestamps, sid, ch, dir, optional level/tags/source/host/schema_id)
```

The `body` for a `datagram` is the captured bytes only. There is **no**
`original_len` (or `captured_len`) field in the live wire payload, so the
frontend has nothing to compare `body.length` against and cannot distinguish
"this packet was 60 bytes" from "this packet was 1514 bytes, clipped to 60".

Adding a length field to the live payload changes the frozen v0.1
`tracemux.v1` wire protocol — a CRITICAL path that requires an ADR, a version
decision, and a compatibility fixture (`tests/compat/wire/*`). This ADR
captures that decision so the work is not done ad hoc inside an unrelated PR.

Forces at play:

- **Compatibility:** the `tracemux.v1` data payload is frozen; existing
  `tracemux.v1` clients must keep working.
- **Parity:** offline/export views already show truncation; the live view
  should not be strictly less informative.
- **Performance:** the field must be cheap (a single integer) and only present
  when meaningful, to avoid bloating every datagram frame.
- **Minimalism:** prefer the smallest additive change that the existing
  forward-compatibility rule ("unknown fields are ignored") already permits.

## Decision

Add a single **optional, additive** field to the wire `data` payload:

```text
orig_len?: u64   // best-known original on-wire length before snaplen truncation
```

Semantics and rules:

1. `orig_len` is **optional** and only emitted when it is both known and
   greater than the body length (i.e. truncation actually occurred). When
   `orig_len` is absent, the body length is authoritative and no truncation is
   implied. This keeps the common, untruncated case byte-for-byte identical to
   today's frames.
2. `orig_len` is **purely additive**: per the existing wire-protocol rule that
   "unknown fields are ignored", current `tracemux.v1` clients keep working
   unchanged. The subprotocol identifier stays `tracemux.v1`; the **app**
   (capability) version is bumped so newer UIs can advertise that they render
   truncation.
3. For the `datagram` kind produced by the pcap runner, the server populates
   `orig_len` from `packet.original_len` when `packet.original_len >
   packet.captured_len`. Other source kinds never set it.
4. The frontend treats a `data` frame with `orig_len > body.length` as
   truncated and renders `body.length / orig_len` (matching the existing
   `capturedLen / originalLen` rendering used in export/replay views).

The field name `orig_len` (not `original_len`) mirrors the existing terse wire
field style; the on-disk `frames.jsonl` keeps its descriptive `captured_len` /
`original_len` names unchanged (no log-format change required).

## Consequences

- Positive:
  - Live packet views reach parity with offline/export views for truncation.
  - The change is additive and backward compatible; old clients ignore the new
    field, and untruncated frames are unchanged.
  - No log-format change; `frames.jsonl` already carries both lengths.
- Negative:
  - Touches a frozen critical path (`wire.rs`, `wire-protocol.md`), so it needs
    human review, an app-version bump, and a wire compatibility fixture.
  - A trivial amount of extra bytes on truncated datagram frames only.
- Compatibility impact:
  - **wire:** additive optional field; `tracemux.v1` subprotocol unchanged;
    app capability version bumped. New fixture under `tests/compat/wire/`
    proving an old decoder ignores `orig_len` and a new decoder reads it.
  - **log:** none (existing `captured_len`/`original_len` retained).
  - **cli:** none required; the JSON `watch` output MAY surface `orig_len`
    additively in a later change.
  - **app:** UI gains truncation rendering on the live path.

## Alternatives considered

1. **Encode truncation inside the `datagram` body as a map** (e.g.
   `body: { bytes, orig_len }`) — rejected: `datagram` body is a `bin` today;
   switching it to a `map` is a breaking shape change for every datagram
   consumer, not an additive one.
2. **Send a side-channel `ctl` event per truncated packet** — rejected:
   per-packet `ctl` traffic is wasteful, races the data frame, and has no
   stable record locator to bind to (same limitation noted in ADR-0002).
3. **Leave live truncation unsignalled; rely on export/replay** — rejected as
   the long-term answer: it permanently makes the live view less informative
   than the recorded view, which is the gap this ADR closes. (It remains the
   accepted *interim* behaviour until this ADR is implemented.)
4. **Always include `orig_len` on every datagram frame** — rejected: it adds
   bytes to the overwhelmingly common untruncated case and changes existing
   frames byte-for-byte; emitting it only on real truncation keeps the
   common path identical.

## Migration plan

This changes the frozen `tracemux.v1` wire `data` payload (additively), so:

1. Update [docs/protocols/wire-protocol.md](../protocols/wire-protocol.md) to
   document `orig_len?` as an optional additive field with the
   "emit only when truncated" rule.
2. Bump the **app** capability version (the `tracemux.v1` subprotocol string is
   unchanged because the change is additive and ignorable).
3. Add a `tests/compat/wire/` fixture demonstrating (a) a pre-change decoder
   ignores `orig_len` and (b) a post-change decoder reads it.
4. Populate `orig_len` in `crates/server/src/pcap_runner.rs` only when
   `original_len > captured_len`; add the field plumbing in
   `crates/server/src/wire.rs` / the envelope encoder.
5. Render truncation in the web packet views on the live path, reusing the
   existing `capturedLen / originalLen` formatting.
6. Because this touches critical paths, the implementing PR carries the
   `human-review-required` label and is not AI-self-merged.
