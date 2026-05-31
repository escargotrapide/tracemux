# Packet capture MVP notes

This document records the non-normative development scope for adding
Wireshark-style packet capture to tracemux. The accepted MVP requirements are
promoted to `docs/requirements.md`; keep this note as background and rationale.

## Summary

The MVP adds a packet-capture source backed by Npcap/libpcap, records Ethernet
packets with tracemux's server-owned persistence model, exposes capture
statistics and selectable packet views in the UI, and provides pcapng output for
Wireshark-compatible follow-up analysis.

Current implementation status:

- Driver-free code paths, fake backend tests, pcap session-dir writing, pcapng
  export, direct pcapng writing, packet summaries, metrics, detect schema, CLI
  specs, WSS specs, and bounded UI packet views are implemented.
- The native live backend is available behind `pcap-capture` and requires local
  Npcap/libpcap setup before real hardware validation.
- Production acceptance rates are not finalized; high-rate operation should use
  BPF filters, snaplen, and `publish=stats-only`/`sampled` until validated.

The intended initial support level is:

- Windows: supported through Npcap.
- Linux: supported through libpcap.
- macOS: best-effort through the same libpcap backend if validation cost remains
  low.

The expected load class is medium-to-high traffic, with BPF filtering, snaplen
control, bounded queues, and UI throttling available from the first usable
release. The MVP is not a Wireshark replacement and does not aim to ship a full
protocol dissector stack.

## Goals

- Capture Ethernet/IP packets from a selected network interface.
- Support BPF filters for targeted captures and high-rate traffic reduction.
- Preserve original packet bytes losslessly up to the configured snaplen.
- Keep server-side persistence as the source of truth.
- Export captured packets as pcapng.
- Show live capture health and traffic statistics.
- Offer selectable UI modes for statistics, packet list, and packet detail.
- Track kernel drops and application-side drops separately where the backend
  exposes the data.
- Support direct pcapng streaming and rotation for operators that need an
  immediate Wireshark artifact.

## Non-goals

- Replacing Wireshark's full dissector and display-filter engine.
- TCP stream reassembly in the MVP.
- Guaranteed lossless capture at 10 GbE line rate for all packet sizes.
- UI rendering of every packet during high-rate captures.
- UI-side persistence. The server remains responsible for all captured data.

## Platform notes

### Windows

Windows capture uses Npcap. Operators must install Npcap and grant suitable
capture privileges. Interface names may be Npcap device paths rather than
friendly display names, so the UI should display both when available.

### Linux

Linux capture uses libpcap. Operators need either root privileges or appropriate
capabilities such as `CAP_NET_RAW` and `CAP_NET_ADMIN`. Interface names should
be accepted as reported by libpcap.

### macOS

macOS can ride on the libpcap backend if interface enumeration, timestamps, and
permissions validate cleanly. The MVP should not block on macOS-specific UI or
packaging work.

## Capture source

Add a source-only packet capture transport. It should not implement a write-back
sink.

The source spec includes fields equivalent to:

- interface name or device id.
- promiscuous mode.
- snaplen.
- capture buffer size.
- read timeout.
- immediate mode.
- BPF filter.
- save mode.
- optional pcapng output path.
- optional rotation size and duration.

A URL-shaped example could be:

`pcap://Ethernet?snaplen=262144&promisc=1&filter=tcp%20port%20502&save=session`

The current URI syntax is parsed by CLI, WSS control payloads, and the web UI
source-spec helper.

## Timestamp handling

Each packet must preserve tracemux's dual timestamp model:

- `ts_origin`: timestamp reported by Npcap/libpcap for the packet.
- `ts_ingest`: timestamp when the tracemux server received/enqueued the packet.

The capture source should also report clock quality and source metadata so later
analysis can distinguish packet timestamps from server ingest timestamps.

## Storage modes

The MVP implements `session`, `pcapng`, and `both` storage modes.

| Mode | Description | Benefits | Trade-offs |
| --- | --- | --- | --- |
| `session` | Store packets in tracemux session-dir and export pcapng later. | Fits existing server-owned persistence, replay, annotations, and tests. | Requires export before opening in Wireshark. Large exports may take time. |
| `pcapng` | Write pcapng while capturing. | Produces a Wireshark artifact immediately and avoids later conversion. | Needs robust streaming writer, partial-file handling, and rotation. |
| `both` | Write session-dir and pcapng concurrently. | Best operator experience and auditability. | Higher I/O load and more consistency handling. |

Implemented rollout:

1. `session` plus pcapng export.
2. direct `pcapng` streaming.
3. `both` with rotation and startup failure checks.

## pcapng output

pcapng export or streaming output should write at least:

- Section Header Block.
- Interface Description Block with Ethernet link type (`DLT_EN10MB`, value 1).
- Enhanced Packet Block per captured packet.
- captured length and original length.
- timestamp resolution and timestamp values.
- optional interface name and filter metadata when available.
- optional statistics blocks or comments for drop counters when practical.

Exporter behavior should preserve packet order as recorded in the session index.

## UI scope

The UI should allow the operator to choose how much detail to render.

### Statistics mode

Always safe to enable. Show:

- status.
- interface.
- filter.
- capture duration.
- packets captured.
- bytes captured.
- kernel drops.
- application drops.
- packets per second.
- bytes per second.
- writer queue depth.
- current output path.
- output file size when pcapng writing is active.
- last packet timestamp.

### Packet list mode

Show a bounded, virtualized list. Do not render unbounded packet history in the
browser.

Suggested columns:

- sequence number.
- timestamp.
- source.
- destination.
- protocol.
- captured length.
- original length.
- short info text.

The list should support pause/follow behavior and configurable retention count.

### Packet detail mode

Show details for the selected packet only. MVP detail can include:

- Ethernet header.
- IPv4 or IPv6 summary.
- TCP, UDP, or ICMP summary.
- payload hex dump.
- ASCII preview.

Industrial or embedded protocols should be added as explicit decoders after the
packet capture path is stable.

## Metrics

Expose capture metrics through the existing metrics path. Candidate fields:

- `packets_total`.
- `bytes_total`.
- `dropped_kernel_total`.
- `dropped_app_total`.
- `pps`.
- `bytes_per_sec`.
- `capture_queue_depth`.
- `writer_queue_depth`.
- `writer_lag_ms`.
- `pcapng_output_bytes`.
- `rotations_total`.

Metric names should be finalized with the server metrics conventions before
implementation.

## Performance requirements

The initial target is medium-to-high traffic rather than unrestricted line-rate
capture.

Acceptance criteria should be measured with:

- BPF filter enabled and disabled.
- default snaplen and a reduced snaplen.
- packet list enabled and disabled.
- pcapng export path.
- direct pcapng path after it exists.

The design should make high-rate operation possible by:

- separating capture, persistence, and UI fan-out.
- using bounded queues with explicit counters.
- batching disk writes where possible.
- avoiding per-packet UI updates at high rates.
- making packet detail decode lazy.
- exposing drops instead of hiding overload.

## Acceptance criteria draft

- The operator can list capture interfaces on Windows and Linux.
- The operator can start and stop a capture on a selected interface.
- The operator can provide a BPF filter.
- The source records packet bytes with captured length and original length.
- The source records both packet timestamp and server ingest timestamp.
- The server persists captured packets under a session-dir.
- The UI shows live status, packet count, byte count, rate, and drop counters.
- The UI can show a bounded packet list without blocking capture.
- The UI can show details for a selected packet.
- The CLI or server can export the session as pcapng.
- The exported pcapng opens in Wireshark and contains the captured packets in
  order.
- Stress tests report drops explicitly instead of silently losing data.

## Implementation phases

These phases are implemented in the current branch. The remaining work is
operational validation and product hardening.

1. Add the packet capture source behind a feature flag if needed.
2. Add interface enumeration and source configuration parsing.
3. Persist captured packets into session-dir with dual timestamps.
4. Add capture metrics and source status payloads.
5. Add UI statistics mode.
6. Add pcapng exporter.
7. Add bounded packet list mode.
8. Add packet detail and hex view.
9. Add direct pcapng streaming mode.
10. Add rotation, recovery checks, and stress fixtures.

## Remaining questions

- What exact pps and byte-rate targets should be used for acceptance tests?
- What default snaplen should release packaging document for each backend, if it
  differs from the current UI default of `65535`?
- Which industrial protocols should receive first-class decoders first?
- Should macOS be marked supported immediately or experimental until manually
  validated?
