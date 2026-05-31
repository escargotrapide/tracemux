# Packet capture requirements draft

This document proposed requirements for the packet capture MVP. The accepted
MVP wording has been promoted to `docs/requirements.md`; keep this file as the
review history and rationale for any later packet-capture requirement changes.

Changes to the promoted requirements still touch a critical path and require
human review.

## Proposed functional requirements

### FR-SRC-PCAP  Packet capture source

`tracemux` provides a source-only packet capture transport backed by
Npcap/libpcap. The source opens a selected capture interface and captures link
layer packets according to the configured options.

The source accepts at least:

- interface identifier.
- promiscuous mode flag.
- snaplen.
- BPF filter.
- capture timeout.
- capture buffer size when supported by the backend.
- UI publish mode.
- storage mode.

The source does not implement write-back `Sink`.

Acceptance criteria:

- Starting a pcap source registers a server-owned session with kind `pcap`.
- A valid BPF filter is applied before packet capture begins.
- An invalid BPF filter fails source startup with a public `E-NNNN` error.
- Captured packets preserve captured length and original length.
- Captured packets preserve packet-origin timestamp and server-ingest timestamp.
- Stopping the source closes the capture handle and leaves persisted session
  data readable.
- The source reports kernel/backend drops when the backend exposes them.

### FR-SRC-PCAP-DETECT  Packet interface discovery

The server can discover packet capture interfaces available to the host and
return enough metadata for the UI to let an operator select the intended
interface.

Acceptance criteria:

- Discovery returns stable device identifiers.
- Discovery returns display names or descriptions when available.
- Discovery can return interface addresses when policy allows them.
- Discovery failure is reported without crashing the server.
- If interface metadata is exposed through HTTP, the endpoint is reviewed for
  authentication and privacy impact.

### FR-LOG-PCAP  Packet session-dir persistence

Packet capture sessions are persisted in the server-owned session-dir format.
Each stored packet writes packet bytes to `raw.bin`, an envelope row to
`index.jsonl`, and packet metadata to an existing structured-record file.

Acceptance criteria:

- `raw.bin` contains captured packet bytes, not UI-derived data.
- `index.jsonl` contains one `kind = "datagram"` row per stored packet.
- `index.jsonl` stores `ts_origin` from the capture backend packet timestamp.
- `index.jsonl` stores `ts_ingest` from the tracemux server.
- Structured metadata includes sequence number, captured length, original length,
  link type, interface id, raw offset, and raw length.
- Persisted data remains readable after stop, EOF, or controlled source error.
- The browser never writes or owns packet persistence.

### FR-EXP-PCAPNG  pcapng exporter

`tracemux export pcapng <session-dir> <dst>` and the authenticated server export
endpoint can render a packet capture session-dir as pcapng.

Acceptance criteria:

- CLI export accepts `pcapng` as an exporter kind.
- HTTP export accepts `format=pcapng` for server-known persisted sessions.
- Export refuses inputs that do not look like session-dirs.
- Export resolves HTTP session paths through `SourceManager`, not arbitrary
  client-supplied filesystem paths.
- The pcapng output contains a Section Header Block.
- The pcapng output contains Interface Description Blocks for captured link
  types/interfaces.
- The pcapng output contains one Enhanced Packet Block per exported packet.
- Enhanced Packet Blocks preserve captured length and original length.
- Packet timestamps in pcapng are derived from `ts_origin`.
- The resulting pcapng opens in Wireshark for Ethernet captures.

### FR-DEC-PACKET-SUMMARY  Packet summary decoder

The server can produce lightweight packet summaries for common Ethernet/IP
traffic without attempting full Wireshark-style dissection.

Acceptance criteria:

- Ethernet II source and destination MAC addresses are summarized when present.
- VLAN tags are summarized when present and supported by the parser.
- IPv4 and IPv6 source/destination addresses are summarized when present.
- TCP and UDP ports are summarized when present.
- ICMP/ICMPv6 protocol summaries are supported at a basic level.
- Malformed or truncated packets do not panic the parser.
- Unsupported protocols preserve raw bytes and report a generic protocol label.

### FR-UI-PCAP  Packet capture UI

The web UI provides packet capture controls and selectable views for pcap
sources.

Acceptance criteria:

- The operator can choose an interface from discovered candidates.
- The operator can enter a BPF filter.
- The operator can set snaplen and promiscuous mode.
- The operator can choose statistics-only, packet-list, or packet-detail display.
- The UI shows capture status, packet count, byte count, rate, and drop counters.
- The packet list is bounded or virtualized and does not render unbounded
  history.
- Packet detail is decoded lazily for the selected packet.
- Statistics-only mode does not subscribe to raw packet data.
- Export controls include pcapng for persisted pcap sessions.

### FR-CLI-PCAP  CLI pcap source spec

CLI subcommands that accept source specs can parse pcap source specs and open the
pcap source when the real backend feature is available.

Acceptance criteria:

- URI-style pcap source specs parse into `ChannelSpec::Pcap`.
- Parsed pcap specs can be rendered back to a filesystem-safe session kind and
  interface tag.
- Without the real capture backend feature or required OS driver, opening the
  source fails with a clear `E-1101` source-open error.
- Existing non-pcap source spec behavior remains unchanged.

### FR-MET-PCAP  Packet capture metrics

The server publishes packet capture metrics suitable for live status panels and
headless monitoring.

Acceptance criteria:

- Metrics include packet count and byte count.
- Metrics include kernel/backend drop count when available.
- Metrics include application-side drop count.
- Metrics include capture and writer queue depth when queues are enabled.
- Metrics include packet rate and byte rate or enough counters for the UI to
  derive them.
- Metrics updates do not require streaming raw packet data to the browser.

## Proposed non-functional requirements

### NFR-PERF-PCAP  Medium-to-high rate capture

Packet capture supports medium-to-high rate traffic while preserving stored data
within configured queue, disk, and backend limits.

Acceptance criteria:

- Persistence and UI fan-out are separate so a slow browser cannot block packet
  storage.
- Packet-list UI can be disabled for high-rate captures.
- Overload increments explicit drop counters instead of silently hiding loss.
- BPF filter and snaplen are available from the first usable release.
- Stress tests cover at least fake-backend high-rate capture with packet-list
  disabled.
- Final pps and byte-rate acceptance numbers are selected before implementation
  is declared production-ready.

### NFR-REL-PCAP  Capture error handling and recoverability

Packet capture failure modes are reported clearly and leave durable artifacts in
a recoverable state.

Acceptance criteria:

- Missing Npcap/libpcap or missing permissions produce a clear source-open
  error.
- Invalid interface identifiers produce a clear source-open error.
- Invalid BPF filters produce a clear source-open error.
- pcapng export failures do not corrupt the source session-dir.
- Direct pcapng streaming, when implemented, reports partial artifact failure to
  the UI.
- Stop/remove operations are idempotent from the user's perspective.

### NFR-SEC-PCAP  Packet capture security and privacy

Packet capture does not weaken existing server-owned persistence, authentication,
or privacy boundaries.

Acceptance criteria:

- UI clients never persist packet bytes locally except through explicit browser
  downloads initiated by the user.
- HTTP export remains authenticated unless loopback no-auth policy applies.
- Interface discovery is reviewed because interface names and addresses may leak
  host network information.
- Source specs must not embed secrets.
- Error messages do not dump packet payloads.

### NFR-PORT-PCAP  Packet capture portability

Packet capture is portable across the intended desktop/server platforms with
clear support levels.

Acceptance criteria:

- Windows x64 support is based on Npcap.
- Linux x64 support is based on libpcap.
- macOS support uses libpcap and is marked supported only after manual
  validation.
- Normal CI can run without live capture hardware by using a fake backend.
- Real backend tests are manual or environment-gated.

### NFR-MAINT-PCAP  Dependency and review policy

Packet capture dependencies are introduced incrementally and remain compatible
with repository policy.

Acceptance criteria:

- pcapng export uses an allowed-license dependency or an in-tree writer reviewed
  for pcapng correctness.
- Live capture dependency changes run `cargo deny` successfully.
- `openssl-sys` is not introduced.
- Dependencies that require OS drivers are feature-gated or otherwise isolated
  from driver-free CI.
- Critical-path files touched by packet capture are called out for human review.

## Proposed requirement promotion order

1. Promote `FR-EXP-PCAPNG` and exporter tests if pcapng export is implemented
   before live capture.
2. Promote `FR-SRC-PCAP`, `FR-LOG-PCAP`, and `FR-CLI-PCAP` when adding the fake
   backend and pcap session writer.
3. Promote `FR-UI-PCAP` and `FR-MET-PCAP` when UI statistics/list work begins.
4. Promote `NFR-PERF-PCAP`, `NFR-REL-PCAP`, `NFR-SEC-PCAP`,
   `NFR-PORT-PCAP`, and `NFR-MAINT-PCAP` before real backend PRs.

## RTM notes

After accepted requirements are moved to `docs/requirements.md`:

- Add `// REQ: ...` comments in Rust tests.
- Add equivalent comments in TypeScript tests if UI behavior is covered.
- Run `just rtm` to regenerate `docs/rtm.md`.
- Run `just ai-verify` before review.
