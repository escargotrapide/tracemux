# Packet capture implementation plan

This document decomposes the packet capture MVP into reviewable implementation
slices. It depends on the drafts in:

- `docs/dev/packet-capture-mvp.md`
- `docs/dev/packet-capture-design.md`
- `docs/dev/packet-capture-dependencies.md`
- `docs/dev/packet-capture-requirements-draft.md`

The plan intentionally separates driver-free work from real Npcap/libpcap work
so CI can stay deterministic while the architecture is validated.

## Current branch status

This branch implements PR 1 through PR 11 from the sequence below:

- pcapng export and authenticated HTTP/CLI/UI export wiring.
- lightweight packet summary parsing with `etherparse`.
- pcap config/model types, fake backend, and optional native `pcap-capture`
  backend.
- CLI, WSS control, and web source-spec parsing for `pcap://...`.
- metadata-preserving pcap runner and session-dir writer.
- pcap metrics payloads.
- pcap interface discovery schema and feature-gated native enumeration.
- UI pcap start controls, bounded packet list, detail, and hex preview.
- direct pcapng streaming for `save=pcapng|both` with size/duration rotation.

The remaining items are review/operations work rather than missing MVP code:
human review for critical paths, real Npcap/libpcap smoke tests on configured
hosts, packaging decisions for enabling `pcap-capture`, and final production
pps/byte-rate targets.

## Ground rules

- Do not change frozen trait method signatures.
- Avoid changes to `crates/server/src/wire.rs` unless an ADR is accepted.
- Treat `crates/core/src/source/mod.rs`, `Cargo.lock`, and protocol documents as
  critical paths requiring human review.
- Add tests next to code and reference accepted requirement ids only after those
  ids are promoted to `docs/requirements.md`.
- Prefer fake backend tests before live-interface tests.
- Run `scripts/check-encoding.ps1` after every documentation-heavy slice on
  Windows.

## Recommended PR sequence

### PR 0: promote accepted requirements

Purpose:

- Move the reviewed subset of `packet-capture-requirements-draft.md` into
  `docs/requirements.md`.
- Regenerate RTM after tests exist, or note that RTM links will arrive with the
  implementation PRs.

Primary files:

- `docs/requirements.md` critical.
- `docs/rtm.md` generated.

Tests / checks:

- `just rtm` after test references exist.
- `pwsh scripts/check-encoding.ps1`.

Review notes:

- This PR touches critical docs. Keep wording stable and implementation-neutral.
- If requirements are not ready, skip PR 0 and keep implementation tests without
  `REQ` comments until promotion.

### PR 1: pcapng exporter foundation

Purpose:

- Add pcapng export from a synthetic pcap-shaped session-dir.
- Avoid live capture and avoid `ChannelSpec::Pcap` in this first code slice if
  the team wants minimal critical-path exposure.

Primary files:

- `Cargo.toml` / `crates/core/Cargo.toml` for `pcap-file = "2.0"`.
- `Cargo.lock` critical.
- `crates/core/src/exporter/pcapng.rs` new.
- `crates/core/src/exporter/mod.rs` add module.
- `crates/cli/src/cmd/export.rs` add `pcapng` kind.
- `crates/server/src/export_api.rs` add `format=pcapng`.
- `web/src/adapters/sessionExport.ts` add `pcapng` format.
- `web/src/panels/sources/SourcesPanel.tsx` expose pcapng export button.

Implementation notes:

- Read `index.jsonl`, `raw.bin`, and optional pcap packet metadata from
  `frames.jsonl`.
- Generate pcapng Section Header, Interface Description, and Enhanced Packet
  blocks.
- Use Ethernet link type as fallback only for clearly pcap-origin sessions.
- Prefer `application/vnd.tcpdump.pcapng`, with `application/octet-stream` as
  fallback if needed.

Tests:

- Core unit test creates a tiny synthetic packet session-dir and exports pcapng.
- Exporter test validates block structure with `pcap-file` reader or existing
  importer where practical.
- CLI test verifies unknown/export kinds and `pcapng` dispatch.
- Server export API test verifies `format=pcapng` works for a known session.
- Web unit test verifies pcapng filename extension and URL format.

Checks:

- `cargo fmt`.
- `cargo test -p tracemux-core exporter::pcapng` or nearest exact test target.
- `cargo test -p tracemux-cli` for export dispatch.
- `cargo test -p tracemux-server export_api` if available.
- `pnpm --dir web test -- --run` if web tests are changed.
- `cargo deny check` because dependencies changed.

Risks:

- `Cargo.lock` critical-path review.
- pcapng timestamp resolution mapping must be explicit.

### PR 2: packet summary parser

Purpose:

- Add lightweight L2-L4 packet summary support using `etherparse`.
- Keep this independent of live capture.

Primary files:

- `Cargo.toml` / `crates/core/Cargo.toml` for `etherparse = "0.20"`.
- `Cargo.lock` critical.
- `crates/core/src/decoder/packet_summary.rs` new, or a non-decoder helper module
  if the public decoder surface should remain smaller.
- `crates/core/src/decoder/mod.rs` if added as a module.

Implementation notes:

- Parse Ethernet II, optional VLAN, IPv4, IPv6, TCP, UDP, ICMP, and ICMPv6.
- Return a compact summary struct suitable for JSON fields.
- Treat malformed/truncated packets as summary errors, not panics.
- Do not implement stream reassembly.

Tests:

- Synthetic Ethernet/IPv4/TCP packet.
- Synthetic Ethernet/IPv4/UDP packet.
- Synthetic IPv6 packet.
- Truncated packet.
- Unsupported ethertype fallback.

Checks:

- `cargo fmt`.
- `cargo test -p tracemux-core packet_summary`.
- `cargo deny check` because dependencies changed.

Risks:

- If added as a formal `Decoder`, follow `.github/skills/add-decoder/SKILL.md`.
- Keep summary fields stable enough before writing fixtures around them.

### PR 3: pcap model and fake backend

Purpose:

- Add packet capture config/model types and a fake backend without real OS
  capture dependencies.
- Prepare deterministic server tests.

Primary files:

- `crates/core/src/source/pcap.rs` new.
- `crates/core/src/source/mod.rs` critical, only if `ChannelSpec::Pcap` is added
  in this PR.
- `crates/core/tests/source_pcap.rs` or unit tests next to module.

Implementation notes:

- Define `PcapConfig`, `PcapPacket`, `PcapStats`, `PcapPublishMode`, and
  `PcapSaveMode`.
- Define an internal `PcapBackend` trait.
- Implement `FakePcapBackend` for tests.
- Implement `PcapSource` as source-only.
- Map `Source::recv()` to `Frame::Datagram` for compatibility, but expose a
  metadata-preserving `recv_packet()` for the pcap runner.

Tests:

- Fake backend emits deterministic packets.
- `PcapSource::metadata()` returns kind `pcap` and expected interface label.
- `Source::recv()` compatibility path returns datagram bytes.
- `recv_packet()` returns original length, captured length, link type, and
  timestamp.

Checks:

- `cargo fmt`.
- `cargo test -p tracemux-core source_pcap` or exact module tests.

Risks:

- Adding `ChannelSpec::Pcap` touches a critical frozen config surface.
- Keep trait signatures unchanged.

### PR 4: pcap source spec parsing and lifecycle wiring

Purpose:

- Make pcap specs parseable by CLI, WSS control, and web UI.
- Do not add the real backend yet.

Primary files:

- `crates/cli/src/cmd/spec.rs`.
- `crates/server/src/ws.rs`.
- `crates/server/src/source_manager.rs`.
- `web/src/state/sourceSpec.ts`.
- tests under `crates/cli`, `crates/server`, and `web/tests/unit`.

Implementation notes:

- Support URL form such as
  `pcap://Ethernet?snaplen=65535&promisc=1&filter=tcp%20port%20502`.
- Add filesystem-safe kind/interface tags.
- WSS `payload.spec.kind = "pcap"` should construct the same config fields.
- Without real backend, opening can use fake backend only in tests or return a
  clear feature-gated source-open error in production builds.

Tests:

- CLI parse/render pcap spec.
- CLI rejects invalid numeric values.
- WSS `channel_spec_from_value()` accepts pcap maps.
- Web `parseSourceSpec()` accepts pcap URI and emits expected JSON shape.
- Non-pcap parsing tests continue to pass.

Checks:

- `cargo fmt`.
- `cargo test -p tracemux-cli spec`.
- `cargo test -p tracemux-server ws`.
- `pnpm --dir web test -- --run sourceSpec` if test filtering exists.

Risks:

- `source_manager.rs` should route pcap to a pcap-specific runner, not the
  generic runner, otherwise packet metadata is lost.

### PR 5: pcap-specific runner and session-dir writer

Purpose:

- Persist packets from fake backend to session-dir with dual timestamps and
  packet metadata.

Primary files:

- `crates/server/src/pcap_runner.rs` new.
- `crates/server/src/source_manager.rs` route pcap startup.
- `crates/server/src/lib.rs` or module declarations.
- `crates/server/tests/pcap_source.rs` new integration test.

Implementation notes:

- Register a session with kind `pcap`.
- Write packet bytes to `raw.bin`.
- Write `Kind::Datagram` rows to `index.jsonl`.
- Write metadata summary to `frames.jsonl` with schema id
  `tracemux.pcap.packet.v1`.
- Use packet `ts_origin` and server `ts_ingest`.
- Commit and close writers on normal completion.
- Keep UI publish configurable; default can be statistics-only or sampled.

Tests:

- Start fake pcap source through `SourceManager`.
- Wait for completion and inspect session-dir files.
- Assert `index.jsonl.kind == "datagram"`.
- Assert `ts_origin != ts_ingest` is preserved when fake timestamps differ.
- Assert `frames.jsonl` metadata includes original/captured length and raw
  offset/length.
- Assert pcapng exporter can export the generated session.

Checks:

- `cargo fmt`.
- `cargo test -p tracemux-server pcap`.
- `cargo test -p tracemux-core exporter::pcapng`.

Risks:

- Directly using `RawWriter`, `IndexWriter`, and `FramesWriter` duplicates part
  of `FileLogSink`; keep it narrowly scoped to pcap metadata preservation.

### PR 6: pcap metrics and source status

Purpose:

- Expose packet capture counters without requiring packet-list subscription.

Primary files:

- server metrics publisher location, to be finalized from current metrics
  architecture.
- `crates/server/src/pcap_runner.rs`.
- `web/src/panels/metrics/MetricsPanel.tsx` only if display needs custom
  formatting; flat metrics already render generically.
- tests for metrics snapshots.

Implementation notes:

- Publish packet count, byte count, backend drops, app drops, queue depth, and
  rates.
- Keep source-list payload small.
- Metrics must continue after UI reconnect.

Tests:

- Fake backend produces packets and metrics counters advance.
- Drop counters are surfaced when fake backend reports drops.
- Metrics frame payload contains pcap counters.

Checks:

- `cargo test -p tracemux-server metrics` or pcap-specific test.
- Web unit tests only if UI state changes.

Risks:

- If no reusable metrics broadcaster exists, add one in a non-wire-critical
  module rather than changing `wire.rs`.

### PR 7: pcap interface discovery

Purpose:

- Let UI operators select capture interfaces.

Primary files:

- `crates/core/src/detect/pcap.rs` new.
- `crates/core/src/detect/mod.rs`.
- `crates/server/src/routes.rs` or a new authenticated route module.
- `web/src/state/sourceDiscovery.ts`.
- `web/src/panels/sources/SourcesPanel.tsx`.

Implementation notes:

- Decide whether to add pcap info to public `/api/detect` or create an
  authenticated `/api/pcap/interfaces` route.
- Prefer minimal public data if keeping `/api/detect` unauthenticated.
- Real detection can be feature-gated until `pcap` backend arrives; fake/static
  test data can cover UI flow.

Tests:

- Detect report includes pcap kind when feature is available.
- Interface records normalize and sort in web state.
- UI can select an interface and produce a pcap source spec.

Checks:

- `cargo test -p tracemux-server routes`.
- `pnpm --dir web test -- --run sourceDiscovery` if available.

Risks:

- Interface names and addresses may leak host network details. Security review
  is required before exposing full metadata.

### PR 8: pcap UI statistics mode

Purpose:

- Add operator-friendly pcap start controls and statistics view.

Primary files:

- `web/src/panels/sources/SourcesPanel.tsx` or new pcap start component.
- `web/src/panels/packetCapture/PacketCapturePanel.tsx` new.
- `web/src/App.tsx` register panel if standalone.
- `web/src/i18n/en.json` and `web/src/i18n/ja.json`.
- `web/src/styles.css` if needed.
- web unit tests.

Implementation notes:

- Provide interface selector, filter, snaplen, promiscuous mode, and publish
  mode.
- Statistics mode must not subscribe to raw packet data.
- Use existing metrics state where possible.

Tests:

- Component renders empty state without pcap source.
- Component shows pcap metrics when supplied.
- Start form sends expected `ctl start` payload.
- i18n keys exist for English and Japanese.

Checks:

- `pnpm --dir web test -- --run`.
- `pnpm --dir web lint` if available.

Risks:

- Avoid making SourcesPanel too large; split pcap controls into a child
  component if the file becomes unwieldy.

### PR 9: packet list and detail UI

Purpose:

- Add bounded packet list and selected-packet detail/hex view.

Primary files:

- `web/src/state/packetCapture.ts` new.
- `web/src/panels/packetCapture/PacketCapturePanel.tsx`.
- `web/src/panels/packetCapture/PacketList.tsx` new.
- `web/src/panels/packetCapture/PacketDetail.tsx` new.
- web tests.

Implementation notes:

- Store only a bounded ring in browser memory.
- Use virtualization if rendering more than a small list.
- Decode details lazily for selected packet only.
- Provide pause/follow controls.
- Show captured length and original length.

Tests:

- Ring buffer evicts old packets.
- Packet list renders summary columns.
- Detail panel renders hex and ASCII preview.
- Pause/follow behavior does not keep growing state unbounded.

Checks:

- `pnpm --dir web test -- --run`.

Risks:

- Do not route all high-rate captures to UI by default.

### PR 10: real Npcap/libpcap backend

Purpose:

- Add live packet capture with `pcap = "2.4"` behind a feature.

Primary files:

- `Cargo.toml` / `crates/core/Cargo.toml` for optional `pcap` dependency.
- `Cargo.lock` critical.
- `crates/core/src/source/pcap.rs` real backend implementation.
- setup docs under `docs/dev/` or README section if accepted.
- environment-gated tests.

Implementation notes:

- Feature name example: `pcap-capture`.
- Use blocking capture on a dedicated task/thread unless `capture-stream` proves
  necessary.
- Apply snaplen, promisc, timeout, buffer size, and BPF before activation.
- Read backend statistics for kernel drops.
- Convert backend timestamps to `ts_origin_ns` carefully.
- Return `E-1101` for missing driver, missing permission, invalid interface, and
  invalid BPF filter where appropriate.

Tests:

- Unit tests remain fake-backend based.
- Ignored or env-gated live tests:
  - `TRACEMUX_PCAP_TEST_IFACE`.
  - optional `TRACEMUX_PCAP_TEST_FILTER`.
- Manual Windows Npcap test.
- Manual Linux libpcap/capability test.

Checks:

- `cargo test -p tracemux-core --features pcap-capture` where environment can
  build libpcap/Npcap.
- `cargo deny check`.
- `just ai-verify` on a configured environment.

Risks:

- Build environments without libpcap/Npcap SDK may fail if feature defaults are
  wrong. Keep feature disabled by default unless packaging requirements decide
  otherwise.

### PR 11: direct pcapng streaming and rotation

Purpose:

- Add optional direct pcapng writing during capture for long/high-rate captures.

Primary files:

- `crates/core/src/exporter/pcapng.rs` or new writer module.
- `crates/server/src/pcap_runner.rs`.
- UI controls for save mode and output path if server policy allows.
- tests for rotation/recovery.

Implementation notes:

- Support `save=pcapng` and `save=both` only after `save=session` is stable.
- Add rotation by file size and/or duration.
- Surface partial artifact errors to UI.
- Keep session-dir as the default unless product owners choose otherwise.

Tests:

- Direct pcapng file is written for fake packets.
- Rotation creates expected filenames.
- Simulated writer error increments error state and does not corrupt session-dir.

Checks:

- `cargo test -p tracemux-server pcapng`.
- Manual long-running capture smoke test.

Risks:

- Higher I/O load in `save=both` mode. Make this explicit in UI.

## Cross-cutting checklist

For every code PR:

- Add or update tests before declaring done.
- Keep critical-path changes isolated and easy to review.
- Preserve dual timestamps.
- Verify raw packet bytes stay server-owned.
- Ensure UI state is bounded.
- Run relevant Rust and web tests.
- Run `pwsh scripts/check-encoding.ps1` on Windows.
- Run `just rtm` after accepted REQ ids are referenced.
- Run `just ai-verify` before requesting review.

## Suggested first implementation milestone

The smallest useful milestone is:

1. `pcap-file = "2.0"` pcapng exporter.
2. synthetic pcap session-dir fixture/test.
3. CLI and HTTP `format=pcapng` export.
4. web export format support.

This milestone produces a Wireshark-compatible artifact without requiring
Npcap/libpcap installation. It also validates the session-dir-to-pcapng mapping
before live capture adds driver and permission complexity.

## Post-implementation decisions and follow-ups

- `FR-EXP-PCAPNG` and the rest of the accepted packet capture MVP requirements
  are promoted in `docs/requirements.md`; keep RTM regenerated after changing
  requirement references.
- pcapng export is always built because `pcap-file` is pure Rust. Live capture
  remains feature-gated behind `pcap-capture` because native drivers/SDKs are
  host-specific.
- Interface discovery currently uses additive `/api/detect` fields and omits
  addresses from the public payload. Revisit this if authenticated discovery is
  added.
- The UI default snaplen is `65535` to match common libpcap defaults and avoid
  surprising artifact size increases.
- Final production pps and byte-rate targets are still open and require real
  hardware validation. Existing tests cover deterministic fake-backend behavior,
  persistence, metrics, export, direct pcapng writing, and bounded UI state.
