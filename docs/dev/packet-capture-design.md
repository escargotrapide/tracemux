# Packet capture technical design draft

This document turns `docs/dev/packet-capture-mvp.md` into an implementation
oriented design. It is still non-normative. Before implementation, promote the
stable parts into `docs/requirements.md`, update RTM, and expect human review
for critical paths such as `crates/core/src/source/mod.rs`.

## Design principles

- Keep the server as the source of truth for capture, persistence, metrics, and
  export.
- Do not change frozen trait signatures or the `wanlogger.v1` frame type set.
- Preserve packet-origin timestamps and server-ingest timestamps.
- Persist every captured packet selected for storage, but make UI fan-out
  configurable and bounded.
- Prefer additive JSON and UI changes over wire-protocol changes.
- Keep packet capture source-only. It must not implement write-back `Sink`.

## Important existing constraints

The generic source runner in `crates/server/src/runner.rs` currently stamps a
frame after `Source::recv()` returns by calling `TimeSource::stamp_origin()` and
then `TimeSource::stamp_ingest()`. The frozen `Source::recv()` API returns only a
`Frame`, so it cannot explicitly return packet metadata such as libpcap packet
timestamp, original length, link type, or interface id.

That means a naive `PcapSource -> generic runner -> FileLogSink` path would lose
important pcap metadata or would have to encode it into the packet bytes. The
MVP should avoid that. Packet capture should use a pcap-specific runner that can
receive packet metadata and write session-dir rows directly without changing the
frozen traits.

## Core types

Add a new source module:

- `crates/core/src/source/pcap.rs`

Proposed internal structs:

- `PcapConfig`
  - `interface: String`
  - `display_name: Option<String>`
  - `promiscuous: bool`
  - `snaplen: u32`
  - `buffer_bytes: Option<u32>`
  - `timeout_ms: u32`
  - `immediate: bool`
  - `filter: Option<String>`
  - `save_mode: PcapSaveMode`
  - `pcapng_path: Option<PathBuf>`
  - `publish_mode: PcapPublishMode`
- `PcapPacket`
  - `seq: u64`
  - `ts_origin_ns: i64`
  - `ts_ingest_ns: i64` is filled by the runner, not the source.
  - `captured_len: u32`
  - `original_len: u32`
  - `linktype: u32`
  - `interface_id: u32`
  - `data: Bytes`
- `PcapStats`
  - `packets_total`
  - `bytes_total`
  - `dropped_kernel_total`
  - `dropped_app_total`
  - `capture_queue_depth`
  - `writer_queue_depth`
  - `last_packet_ts_origin_ns`

`PcapSource` should implement the frozen `Source` trait for compatibility with
existing generic tooling, but the server should use an inherent method such as
`recv_packet()` in the packet-specific runner so metadata is preserved. The
trait implementation can map packets to `Frame::Datagram` for simple consumers.

## ChannelSpec

Add a `Pcap` variant to `ChannelSpec` in `crates/core/src/source/mod.rs`:

- `interface: String`
- `promiscuous: bool`
- `snaplen: u32`
- `buffer_bytes: Option<u32>`
- `timeout_ms: u32`
- `immediate: bool`
- `filter: Option<String>`
- `save_mode: String` initially, or a serializable enum if schema work is done.
- `pcapng_path: Option<String>`
- `publish_mode: Option<String>`

This file is a critical path. Adding a variant is smaller than changing trait
methods, but it still changes the config surface and requires careful review.

Update all existing spec conversion points:

- `crates/cli/src/cmd/spec.rs`
  - parse `pcap://...` URI.
  - render `ChannelSpec::Pcap`.
  - update `kind_tag()` and `iface_tag()`.
  - open `PcapSource` for CLI/extcap compatibility.
- `crates/server/src/ws.rs`
  - accept `payload.spec.kind == "pcap"` in `channel_spec_from_value()`.
- `web/src/state/sourceSpec.ts`
  - parse pcap URI input from the Sources panel.
- `crates/server/src/source_manager.rs`
  - route `ChannelSpec::Pcap` to `start_pcap_spec()`.
  - update `kind_tag()` and `iface_tag()`.

## Backend abstraction

Use a small backend boundary so CI does not require real network interfaces:

- `PcapBackend` trait, internal to `source/pcap.rs` or `source/pcap/backend.rs`.
- `LibpcapBackend` using the `pcap` crate for Windows Npcap, Linux libpcap, and
  best-effort macOS libpcap.
- `FakePcapBackend` for unit tests and deterministic integration tests.

The real backend should be behind a Cargo feature if dependency or CI impact is
high. The fake backend should remain dependency-free.

Npcap/libpcap dependency changes update `Cargo.lock`, which is a critical path.
Run `cargo deny` and confirm the dependency tree does not violate the existing
policy, including the `openssl-sys` ban.

## Pcap runner

Add a packet-specific runner in the server layer, for example:

- `crates/server/src/pcap_runner.rs`

Responsibilities:

1. Open `PcapSource` and register a session.
2. Receive `PcapPacket` values with packet metadata.
3. Stamp `ts_ingest` with the server clock while preserving `ts_origin` from
   libpcap/Npcap.
4. Append packet bytes to `raw.bin`.
5. Append `Kind::Datagram` rows to `index.jsonl`.
6. Append packet metadata and lightweight decode summary to `frames.jsonl`.
7. Optionally publish bounded `data` frames for subscribed packet-list UI.
8. Update capture metrics and drop counters.
9. Commit/close writers on EOF, stop, or error.

This avoids changing `LogSink` while still using existing session-dir files.
`FileLogSink::append_raw()` always writes `Kind::Bytes`, so the pcap runner
should use `RawWriter`, `IndexWriter`, and `FramesWriter` directly.

## Session-dir representation

For each packet:

- `raw.bin` stores the captured packet bytes only.
- `index.jsonl` stores one row with:
  - `kind = "datagram"`.
  - `off` and `len` pointing at the packet bytes in `raw.bin`.
  - `ts_origin` from libpcap/Npcap.
  - `ts_ingest` from the server.
  - `source = "pcap:<interface>"`.
- `frames.jsonl` stores one structured metadata record with:
  - `schema_id = "wanlogger.pcap.packet.v1"`.
  - `fields.seq`.
  - `fields.raw_off`.
  - `fields.raw_len`.
  - `fields.captured_len`.
  - `fields.original_len`.
  - `fields.linktype`.
  - `fields.interface_id`.
  - `fields.interface`.
  - `fields.filter` when set.
  - lightweight L2-L4 summary fields when decoded.

The `raw_off` and `raw_len` fields let the pcapng exporter join metadata to the
matching index row without relying only on line order.

## Lightweight packet decoder

Add a small decoder/utility module for packet summaries. It should parse only
safe, bounded headers from the captured bytes:

- Ethernet.
- VLAN tag, if present.
- IPv4 and IPv6 summary.
- TCP, UDP, and ICMP summary.
- payload offset and payload length.

This module should not attempt full Wireshark-style dissection. Industrial and
embedded protocols should be added later as explicit decoders that operate on
payload bytes or summary records.

A possible location is:

- `crates/core/src/decoder/packet_summary.rs`

If adding it as a formal `Decoder`, follow `.github/skills/add-decoder/SKILL.md`.
If it is only a helper used by `pcap_runner`, keep the public surface minimal.

## pcapng exporter

Add:

- `crates/core/src/exporter/pcapng.rs`

Behavior:

1. Read `index.jsonl` and select `kind == "datagram"` rows from pcap sources.
2. Read `raw.bin` packet bytes by `off` and `len`.
3. Read pcap metadata from `frames.jsonl` records with
   `schema_id == "wanlogger.pcap.packet.v1"`.
4. Join metadata by `raw_off` and `raw_len`.
5. Write pcapng:
   - Section Header Block.
   - Interface Description Block per interface/linktype combination.
   - Enhanced Packet Block per packet.
   - captured length and original length.
   - timestamp values from `ts_origin`.
6. If metadata is missing, fallback to `original_len = len` and Ethernet link
   type only when the session clearly came from pcap.

Wire it into:

- `crates/core/src/exporter/mod.rs` with `pub mod pcapng;`.
- `crates/cli/src/cmd/export.rs` with `pcapng` kind.
- `crates/server/src/export_api.rs` with `format=pcapng`.
- `web/src/adapters/sessionExport.ts` with `SessionExportFormat = "pcapng"`.
- `web/src/panels/sources/SourcesPanel.tsx` export buttons.

Suggested content type:

- `application/vnd.tcpdump.pcapng` if accepted by clients.
- otherwise `application/octet-stream` is a safe fallback.

## HTTP/UI export downloads

The browser UI should use native browser downloads for exported artifacts rather
than reading large responses into `Blob` objects. This is especially important
for pcapng and multi-source ZIP exports, where browser memory pressure can abort
otherwise valid server responses.

Server-side behavior:

- `GET /api/sessions/{sid}/export` streams a temporary export file to the
  response body and removes the temporary file after the stream finishes or is
  dropped.
- Authenticated native downloads use short-lived, one-use tickets from
  `POST /api/sessions/{sid}/export-ticket` so the browser can navigate to a
  normal download URL without sending custom headers.
- Bulk exports are built on the server. The UI requests a one-use ticket with
  `POST /api/exports/bundle-ticket` and then downloads
  `GET /api/exports/bundle?ticket=...` as a ZIP.
- Empty optional parameters from forms, such as timezone or filename pattern,
  are treated as omitted.

UI-side behavior:

- Single-source export buttons call `downloadSessionExport`, which constructs a
  native download URL and triggers browser download navigation.
- "Export all sources" buttons call `downloadSessionExportZip`, which uses the
  server-side ZIP path for production downloads. The client-side ZIP builder is
  kept only for small tests and fallback-style helpers.
- Dev CORS exposes `Content-Disposition`, `Content-Length`, and `Content-Type`
  so browser tooling can inspect export responses during local UI development.

## Direct pcapng streaming

Direct pcapng writing is implemented by `PcapngStreamWriter` and is fed from
the same metadata-preserving `PcapPacket` stream as the session-dir writer. The
writer creates a valid pcapng section immediately, writes interface blocks
lazily per `(linktype, label)`, and rotates before appending the next packet
when size or duration policy says to rotate.

Supported modes:

- `save=session`: session-dir only.
- `save=pcapng`: direct pcapng only. If `pcapng_path` is omitted, the runner
  uses `capture.pcapng` inside the server session directory when one exists.
- `save=both`: session-dir and direct pcapng.

For `both`, pcapng writer failures fail the runner instead of silently
continuing with divergent artifacts. Operators can export from the session-dir
afterward when the session-dir path is available.

## UI data flow

Existing state can be extended without changing the wire frame type set.

### Detection

`GET /api/detect` currently returns transport kinds and serial candidates. Add
pcap fields additively:

- `pcap_interfaces: PcapInterfaceInfo[]`
- `PcapInterfaceInfo.device`
- `PcapInterfaceInfo.display_name`
- `PcapInterfaceInfo.description`
- `PcapInterfaceInfo.addresses`
- `PcapInterfaceInfo.flags`

Because interface names and addresses may be sensitive, the current
implementation keeps addresses empty in `/api/detect`. Add an authenticated
pcap-interface endpoint before exposing fuller host network metadata.

### Source start form

Extend `web/src/state/sourceSpec.ts` and `SourcesPanel` with pcap fields:

- interface selector.
- BPF filter input.
- snaplen.
- promiscuous mode.
- buffer size.
- publish mode.
- save mode.

### Packet views

Add a dedicated panel, for example:

- `web/src/panels/packetCapture/PacketCapturePanel.tsx`

Modes:

- Statistics only: does not subscribe to raw packet data.
- Packet list: subscribes to channel 0 and stores a bounded ring.
- Packet detail: decodes only the selected packet in the browser.

Do not send every packet to the browser unless the user enables packet-list mode.
The pcap runner should support a publish policy such as:

- `stats-only`.
- `sampled`.
- `full`.

The current `DataPayload.kind` type already includes `datagram`, so pcap data
frames can use `kind = "datagram"` and `body = Uint8Array` without adding a new
wire frame type.

## Metrics and status

The core metrics registry exists in `crates/core/src/metrics.rs`, and the wire
protocol already has a `metrics` frame type. If no server metrics broadcaster is
active for pcap yet, add one in the server layer or have the pcap runner publish
through the existing metrics mechanism.

Minimum metrics:

- `pcap.<sid>.packets_total`.
- `pcap.<sid>.bytes_total`.
- `pcap.<sid>.dropped_kernel_total`.
- `pcap.<sid>.dropped_app_total`.
- `pcap.<sid>.capture_queue_depth`.
- `pcap.<sid>.writer_queue_depth`.
- `pcap.<sid>.pps`.
- `pcap.<sid>.bytes_per_sec`.
- `pcap.<sid>.pcapng_output_bytes` when direct pcapng is enabled.

`SourceSnapshot` should remain small, but additive source-list fields can be
considered later for packet-specific counters if the UI needs them before the
first metrics frame arrives.

## Backpressure and overload behavior

The capture path should use explicit bounded queues:

- backend capture queue.
- persistence queue.
- optional UI publish queue.

When overload happens:

- backend/kernel drops should be read from libpcap stats when available.
- application drops must increment `dropped_app_total`.
- UI drops must not imply persistence drops.
- high-rate UI modes should degrade to statistics-only or sampled output.

Persistence should not be blocked by a slow browser. This matches the existing
project rule: lossless pipeline for logging, drop-on-lag pipeline for UI.

## Security and permissions

- Windows requires Npcap and suitable capture privileges.
- Linux requires root or capture capabilities.
- BPF filters should be validated by libpcap before starting capture.
- Interface detection may leak host network details, so authenticate it or keep
  public detect data minimal.
- pcapng export must resolve only server-known session dirs through
  `SourceManager`, matching the existing export API rule.

## Testing strategy

### Unit tests

- CLI `pcap://` parser and renderer.
- Web `parseSourceSpec()` for pcap.
- `channel_spec_from_value()` for WSS start payloads.
- packet summary parser with synthetic Ethernet/IP/TCP/UDP packets.
- pcapng writer block structure.
- pcapng exporter from a synthetic pcap session-dir.

### Integration tests

- `FakePcapBackend` produces deterministic packets.
- server starts `ChannelSpec::Pcap` with fake backend.
- session-dir contains `raw.bin`, `index.jsonl`, `frames.jsonl`, and `meta.toml`.
- export `pcapng` produces an artifact that the minimal importer can read back.
- UI unit tests cover pcap source parsing, export format, and packet ring state.

### Manual or environment-gated tests

- Windows Npcap live interface capture.
- Linux libpcap live interface capture.
- high-rate multicast/broadcast capture with packet-list disabled.
- BPF filter correctness with known test traffic.

## Initial implementation slice

1. Add pcap config types and fake backend.
2. Add `ChannelSpec::Pcap` and parsers.
3. Add pcap-specific runner and session writer.
4. Add pcapng exporter from synthetic sessions.
5. Wire CLI/server/UI pcapng export.
6. Add UI pcap start form and statistics mode.
7. Add packet list and detail panel.
8. Add real Npcap/libpcap backend behind feature/dependency review.
9. Add direct pcapng streaming and rotation.

## Open design decisions

- Whether pcap interface discovery should remain under public `/api/detect` or
  move to an authenticated endpoint.
- Whether direct pcapng streaming belongs in the first implementation PR.
- The default snaplen. `65535` is compatible and under the 1 MiB wire frame
  limit, while larger values need stricter UI publish limits.
- The exact pcap dependency and whether to feature-gate it.
- The first industrial protocols to summarize beyond L2-L4.
