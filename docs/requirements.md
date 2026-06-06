# Requirements

Each requirement has a stable id. Tests and tooling reference ids in
comments (`// REQ: FR-…` for Rust/TS sources, `# REQ: FR-…` for shell
and PowerShell scripts). `docs/rtm.md` is generated from those
references.

Categories:

- **FR-CORE-…** four-layer pipeline & traits
- **FR-SRC-…**  individual sources (serial, tcp, …)
- **FR-SINK-…** write-back sinks
- **FR-FRM-…**  framers
- **FR-DEC-…**  decoders
- **FR-LOG-…**  on-disk session-dir layout
- **FR-IMP-…**  importers
- **FR-EXP-…**  exporters
- **FR-WIRE-…** WSS wire protocol
- **FR-CLI-…**  CLI subcommands
- **FR-UI-…**   UI panels
- **FR-MET-…**  metrics payloads
- **FR-SEC-…**  security
- **FR-AI-…**   AI workflow
- **NFR-PERF-…** performance
- **NFR-REL-…**  reliability
- **NFR-SEC-…**  security non-functionals
- **NFR-USE-…**  usability / a11y / i18n
- **NFR-PORT-…** portability

## v0.1 baseline (frozen)

### FR-CORE-001  Four-layer pipeline
The core crate exposes `Source`, `Sink`, `Framer`, `Decoder`, `LogSink`,
`Importer`, `Exporter`, `TimeseriesSink`, `TimeSource` traits as defined
in their respective `mod.rs` files.

### FR-CORE-002  Dual timestamps
Every persisted `Record` and every wire `data` frame carries
`ts_origin`, `ts_ingest`, `mono_ns`, `boot_id`, `node_id`,
`clock_offset_ms`, `clock_quality`, `drift_ppm`, `clock_source`.

### FR-CORE-003  Error id registry
Every public-facing error has a stable `E-NNNN` id registered in
`crates/core/src/error_id.rs`, with no duplicate codes.

### FR-FRM-LINE  Line framer
The line framer emits one frame per configured line terminator (`LF`,
`CRLF`, `CR`, or auto-detected) and reports overlong buffered input as
`E-1003`.

### FR-SRC-SERIAL  Serial-port source
The `SerialSource` struct opens a serial port using `tokio-serial` when
compiled with the `serial` feature. It must:
- Accept `port`, `baud`, `data_bits` (5–8), `parity` (none/even/odd),
  `stop_bits` (1/2), `flow` (none/hardware/software).
- Emit `Frame::Bytes` chunks on each successful read.
- Emit `ControlEvt::Connected` after a successful `open()`.
- Emit `ControlEvt::Disconnected` on `BrokenPipe`.
- Emit `ControlEvt::Eof` on graceful close or zero-byte read.
- Report errors with `E-1001` (`E1001PipelineGeneric`) or `E-1101`
  (`E1101SourceOpen`) as appropriate.
- Without the `serial` feature, `open()` returns `E-1101` immediately.

### FR-WIRE-001  WSS subprotocol
The server accepts WSS connections with subprotocol `tracemux.v1`
and exchanges MessagePack frames matching `docs/protocols/wire-protocol.md`.

### FR-WIRE-002  Auth
Connections present `bearer.<token>` and the server validates via
`argon2id`. `--no-auth` is rejected unless the peer is `127.0.0.1`
or `::1`.

### FR-LOG-001  Session-dir layout
Sessions are persisted under `{prefix}_{kind}_{iface}_{YYYYMMDD-HHMMSS}/`
matching `docs/protocols/log-format.md`.

### FR-LOG-002  Lossless logger pipeline
The logger path uses bounded `mpsc` + group-commit fsync. No record
loss under backpressure within configured queue depth; on overflow,
the producing `Source` is told to apply flow control or fail closed.

### FR-AI-001  Verification gate
`just ai-verify` produces `target/ai-verify.json` with pass/fail per
step. The server's `/api/ai/verify` returns the same JSON.

### FR-AI-002  Release gate
`just release-gate` (or `scripts/release-gate.{ps1,sh}`) refuses to
release when any of the following hold: dirty git tree, dev/alpha/
beta/rc workspace version, missing CHANGELOG entry for the current
version, missing matching git tag, missing or failed
`target/ai-verify.json`, or `cargo audit` / `cargo deny check`
failures. Exit code is the number of blockers (0 = green). The
`Dev` / `--allow-dev` mode skips the version-string and tag checks
so the gate can be smoke-tested on a development tree.

### FR-SEC-001  Secrets indirection
Configuration files only store `secret://name`. Resolution goes to
the OS keyring.

### NFR-PERF-001  Multi-source viewing
The UI must remain responsive (>30 fps frame budget) with 1000
sources at 1 KiB/s aggregate, using server-side coalescing
(16 ms / 500 ms / 2 s) and tile virtualization (N=16 visible tiles).

### NFR-REL-001  Cross-platform parity
Tier-1 platforms: windows-x64, linux-x64. Tier-2: linux-musl-x64,
linux-aarch64, linux-armv7. All Tier-1 features pass CI on Tier-2
unless explicitly opt-out.

### NFR-SEC-001  Memory safety
`unsafe_code = "deny"` workspace-wide. `cargo deny` bans
`openssl-sys`. Releases pass `cargo audit` clean.

### NFR-PORT-001  Single binary
The CLI / server is a single binary `tracemux`. The Tauri app embeds
or sidecars the same binary.

<!-- New requirements: append below in numerical order. Do not renumber. -->

### FR-UI-001  Web shell
The web UI is a SolidJS application under `web/` that loads a Dockview
grid with `sources`, `metrics`, and `terminal` panels. It connects to
the server via WSS subprotocol `tracemux.v1` and never persists log
data locally.

### FR-UI-002  Terminal panel
The terminal panel renders incoming `data` frames whose `body` is
`Uint8Array` via xterm.js with the WebGL renderer (CPU fallback
allowed). It subscribes to a single `(sid, ch)` and unsubscribes on
unmount.

### FR-UI-003  Sources panel
The sources panel lists every `sid` for which a `data` frame has
arrived, showing kind, channels seen, total bytes received, and the
last `ts_ingest`. Empty state shows a localized hint.

### FR-UI-004  Panel-priority feedback
Visible panels report visibility to the server via the
`panel_priority` frame using `IntersectionObserver`, so the server can
pick the right coalescing bucket (16 ms / 500 ms / 2 s).

### FR-UI-005  Localization
The UI ships English and Japanese strings (`web/src/i18n/{en,ja}.json`)
and defaults to Japanese when `navigator.language` starts with `ja`.

### FR-UI-006  Tauri shell
A Tauri 2 shell under `app-tauri/` wraps the web UI. The shell is
outside the Cargo workspace and is responsible for spawning the
`tracemux serve` sidecar in production builds.

### FR-IMP-001  Plain-text importer
`tracemux import text <src> <dst>` ingests a UTF-8 text file as one
record per `\n`-terminated line and produces a v0.1 session-dir at
`<dst>` with `raw.bin` + `index.jsonl`. Records carry
`clock_quality = imported`, `clock_source = imported`. The CLI refuses
to overwrite a non-empty destination directory.

### FR-EXP-001  Plain-text / CSV / JSONL exporters
`tracemux export {text,csv,jsonl} <session-dir> <dst>` reads the
session-dir's `index.jsonl` + `raw.bin` and writes one row per record
to `<dst>`. The CLI refuses to run when `<session-dir>` lacks an
`index.jsonl` file. `--tz` formats exported timestamp fields in a
fixed display timezone such as `UTC`, `GMT+9`, `+09:00`, or
`Asia/Tokyo` without changing the stored session-dir. Text payloads are
decoded with `--encoding` when supplied, otherwise with session metadata
(`encoding` or `decoder = "utf8-text:<label>"`) and finally UTF-8. The
server exposes the same exporter set through an authenticated
`GET /api/sessions/{sid}/export?format=text|csv|jsonl` endpoint that
resolves `{sid}` to a server-known persisted session-dir rather than
accepting arbitrary filesystem paths; `encoding=<label>` applies the
same explicit text decoding override.

### FR-CLI-001  Import / export round-trip
The CLI guarantees that for any plain-text input file `F`,
`tracemux import text F S` followed by
`tracemux export text S G` produces a `G` whose final whitespace-
trimmed column for each row equals the corresponding line of `F` in
order.

### FR-CLI-002  Wireshark extcap capture
`tracemux extcap --capture --extcap-interface tracemux --fifo PATH
--spec URI` opens a [`Source`] from the given spec, writes a libpcap
classic global header (link-type `DLT_USER0` = 147, snaplen 65535,
microsecond resolution, little-endian) to the FIFO, then emits one
pcap record per inbound frame. Records are truncated to the snaplen
while preserving `orig_len`. The capture loop terminates cleanly when
the source returns `None` or the FIFO peer (Wireshark) closes the
pipe.

### FR-UI-007  Metrics panel
The Metrics panel renders the latest `metrics` wire frame as a flat
key/value table and shows the current connection state. It updates
reactively as new frames arrive; if no metrics frames have been
received it shows an empty-state message.

### FR-UI-008  Source actions
The Sources panel exposes `start` / `stop` / `remove` buttons per row,
which the UI translates into a `ctl` frame (`payload.action`) sent to
the server. The server is the authority — the UI only mirrors the
result.

### FR-UI-009  Toast notifications
Inbound `ctl` frames whose `event` is `error`, `auth_failed`,
`ratelimited`, `disconnected` or `eof` are surfaced as toast
notifications with the matching severity. Toasts are dismissable and
include the `error_id` (E-NNNN) when present.

### FR-UI-010  Terminal TX
The Terminal panel forwards `xterm.onData` keystrokes to the server as
a `write` frame whose payload contains the UTF-8-encoded bytes. The
server is responsible for routing the bytes to the underlying source.

### FR-UI-011  Terminal channel switching
The Terminal panel exposes `sid` / `ch` selectors populated from
`sourcesStore`. Changing either selector tears down the previous
subscription and resubscribes to the newly selected `(sid, ch)`.

### FR-UI-012  Tile grid (16-tile virtualization)
A `TileGridPanel` renders up to 16 mini-terminals (4×4 CSS grid),
each bound to a distinct `(sid, ch)` from `sourcesStore`. Off-screen
tiles report `panel_priority{visible:false}` via `IntersectionObserver`
so the server may switch them to the slow coalescing bucket
(NFR-PERF-001).

### FR-SINK-WIRE  Wire write-back routing
The server handles WSS `write` frames by validating `sid`, `ch`, and
`payload.body`, routing bytes to the `Sink` paired with the running
source session, and returning a `ctl` `write_ack` with the same `seq` on
success. Unknown sessions, source-only sessions, stopped sessions, and
malformed payloads return a `ctl` `error` with an `E-NNNN` code.

### FR-SINK-TCP  TCP write-back sink
TCP sessions started by the server expose a `TcpSink` paired with the
same TCP connection as `TcpSource`. Writes preserve request order and
flush through the TCP write half; closed connections report `E-1102`.

### FR-SINK-UDP  UDP write-back sink
UDP sessions started by the server expose a `UdpSink` paired with the
same UDP socket as `UdpSource`. A write uses `payload.target` when
provided, otherwise the most recent inbound peer. If no target can be
resolved, the server returns a wire validation error.

### FR-SINK-SERIAL  Serial write-back sink
Serial sessions started with the `serial` feature expose a `SerialSink`
paired with the same serial stream as `SerialSource`. Without the
feature, serial write-back fails with `E-1101` and does not register a
partially opened session.

### FR-SINK-PROCESS  Process stdin write-back sink
Process sessions started by the server expose a `ProcessSink` connected
to the child process stdin while `ProcessSource` continues to capture
stdout and stderr.

### FR-CLI-003  Send subcommand
`tracemux send` connects to `tracemux serve` using the
`tracemux.v1` WSS subprotocol, sends bytes from `--text`, `--file`,
`--hex`, or stdin as a `write` frame, and optionally waits for
`write_ack` or `error` before exiting.

### FR-CLI-004  Send text encoding
`tracemux send --text` accepts an `--encoding` option and encodes the
text payload with the selected character encoding before sending the
wire `write` frame. File, hex, and stdin payloads remain raw bytes.

### FR-CLI-005  Log classification tags
`tracemux log` and `tracemux serve` accept one or more substring or
regular-expression classification rules and store matching log-type tags
in persisted session-dir metadata. Matching is performed on decoded text
and does not alter the original raw bytes.

### FR-CLI-006  Serve text encoding
`tracemux serve` accepts a default `--encoding` option for server-side
decoded text records. Captured raw bytes remain lossless; only decoded
`lines.jsonl` / `frames.jsonl` text and downstream classification use
the selected encoding.

### FR-CLI-011  Content detection mode
`tracemux serve` accepts `--detect-mode configured|auto|suggest|off`.
For `auto` and `suggest`, the server samples bounded raw bytes at source
startup without dropping them, scores supported text encodings, evaluates
configured string/regular-expression log-type rules against decoded
sample text, and exposes detection metadata in source snapshots. `auto`
may apply a high-confidence encoding to the server-side decoder;
`suggest` reports candidates without changing the configured encoding;
`configured` preserves existing configured defaults; `off` disables
content detection.

### FR-CLI-007  Session-dir name patterns
`tracemux log` and `tracemux serve` accept a session-dir name pattern
for saved logs. Patterns may use `{prefix}`, `{kind}`, `{iface}`,
`{timestamp}`, and `{unix_ns}` tokens; rendered names are sanitised so
they stay within a single filesystem directory name.

### FR-UI-013  Terminal send box
The Terminal panel includes an explicit send input and button that
encodes text as UTF-8 and sends it as a `write` frame to the selected
`(sid, ch)`. The input is disabled when no source is selected.

### FR-UI-014  Display settings
The web UI exposes display settings for terminal scrollback, tile
scrollback, tile sizing, timestamp/log-type/source metadata prefixes,
display timezone, and display text encoding. Text encoding can be
changed from the terminal toolbar, the Sources row actions, or source
details; source and channel overrides re-render existing browser-side
terminal/tile buffers without restarting the source or storing log data
in the browser. Source-start defaults still apply only to newly started
sources unless the user explicitly restarts a source with that encoding.

### FR-UI-015  Multiple terminal panels
The web UI can open additional independent Terminal panels. Existing
"open terminal" actions continue to focus and retarget the primary
Terminal panel, while "new terminal" actions create a separate panel
bound to the selected `(sid, ch)` without forcing other terminals to
switch channels.

### FR-UI-016  Serial detection and confirmed bulk open
The web UI can request host transport discovery from the server, show
serial/COM candidates to the user, and start only the checked ports
after explicit confirmation. Bulk opening reuses the existing source
`start` control action for each selected serial source.

### FR-UI-017  Source/session notes
The web UI exposes a free-form notes field for each selected
source/session. Notes are stored as browser-side annotations only and
must not persist raw log data outside the server-owned session-dir.

### FR-UI-018  Client display clear and bulk ZIP export
The web UI can clear all browser-side terminal/tile display buffers on
request without sending a server mutation and without deleting
server-owned session-dir logs. The sources panel can export all
server-persisted sources known to the client as one ZIP download. Each
ZIP entry is produced by the existing authenticated per-session export
API in one of the existing formats (`text`, `csv`, `jsonl`, `pcapng`),
while non-persistent sources are excluded from the bulk export set.

### FR-WIRE-003  Lifecycle start-option overrides
WSS `ctl` lifecycle actions support optional start-option fields:
`encoding`, `classifier`, `detection_mode`, and
`session_name_pattern`. `classifier` rules may use either `contains` or
`regex` patterns. `start` applies them to the new source. `restart` may
include the same fields without a new `spec`; supplied fields update
that source's stored lifecycle options, while omitted fields keep the
previous values for future `resume` / `restart` actions. Source-list
snapshots expose the effective server-side decoder metadata for the
source lifetime, including a text `encoding` field when the decoder is
text-based and optional content-detection metadata when detection ran.

### FR-CLI-008  Serve serial bulk startup
`tracemux serve --open-all-serial` starts serial sources automatically
at server startup. When no `--serial-port` values are provided it uses
server-side serial discovery; repeated `--serial-port PORT` values limit
the startup set. Baud, data bits, parity, stop bits, and flow control are
configurable, and a failure to open one port must not prevent attempts
for the remaining ports.

### FR-CLI-012  Configuration file
`tracemux serve --config <path>` and `tracemux export --config <path>`
read a UTF-8 TOML `config_version = 1` configuration file. The v1
server table can provide `bind`,
`session_root`, `encoding`, `detect_mode`, `session_name_pattern`,
`token_phc_files`, TLS listener settings, serial startup defaults, live
WSS delivery pacing, and `require_auth`; named `channels.<name>` entries
provide startup `ChannelSpec` values and optional display labels. The
top-level `export` table provides timezone and encoding defaults for
CLI and HTTP exports, and `retention.keep_days` prunes expired
session-dirs at server startup when non-zero. Explicit CLI flags
override overlapping scalar config values, token PHC files from CLI and
config are combined, unsupported config versions are rejected, plaintext
bearer tokens are not stored in config files, and channel startup
failures are reported without preventing the server from attempting the
remaining configured channels.

### FR-CLI-009  Watch subcommand
`tracemux watch` connects to `tracemux serve` using the
`tracemux.v1` WSS subprotocol, subscribes to a target `--sid` and
`--ch`, decodes inbound `data` frames, and emits one JSONL row per frame
using schema `tracemux/watch-frame/v1`. Binary bodies are represented
losslessly as lowercase hex plus length. `--encoding auto` discovers the
target source's text encoding from the server source snapshot; an
explicit `--encoding LABEL` overrides discovery. Text is included when
the bytes decode without replacement under the selected encoding.

### FR-CLI-010  Connect session save
`tracemux connect <spec> --save <session-dir>` preserves the existing
stdout byte stream while also writing inbound payloads to a v0.1
session-dir containing `meta.toml`, `raw.bin`, and `index.jsonl`. The CLI
refuses to overwrite a non-empty destination directory. `--encoding`
records the text encoding metadata used by later text-like exports and
defaults to UTF-8.

### FR-REMOTE-001  Remote WSS mirror
A server-started `remote` channel spec connects to another tracemux
server using the `tracemux.v1` WSS subprotocol, subscribes to the edge
`sid` / `ch` identified by the remote URL query, mirrors inbound `data`
frames into a local server-owned session-dir, and republishes them under
the local session id for UI, CLI, and AI subscribers. The mirror preserves
the edge `ts_origin` and producing `node_id`, stamps a new central
`ts_ingest`, and proxies local `write` frames back to the edge session.
Bearer credentials for the edge server must be supplied by indirection
such as `token_env` or `token_secret`, not by embedding the token value in
the persisted source spec.

### FR-SRC-PCAP  Packet capture source
`tracemux` provides a source-only packet capture transport for link-layer
packets. The native backend is compiled only with the `pcap-capture` feature
and uses Npcap/libpcap; driver-free builds use a deterministic fake backend for
tests and return a clear source-open error for live pcap sources. The pcap
source accepts interface id, display name, promiscuous mode, snaplen, optional
capture buffer size, timeout, immediate mode, optional BPF filter, storage mode,
optional pcapng path, and UI publish mode. It does not implement write-back.

### FR-SRC-PCAP-DETECT  Packet interface discovery
The server can discover packet capture interfaces and expose a minimal additive
detect payload for UI selection. Records include stable device identifiers and
may include display names, descriptions, addresses, and flags when policy
allows. Default builds without `pcap-capture` return an empty pcap interface
list rather than failing discovery.

### FR-LOG-PCAP  Packet session-dir persistence
Packet capture sessions that use `save=session` or `save=both` persist captured
packet bytes in the server-owned session-dir. Each stored packet writes bytes to
`raw.bin`, a `kind = "datagram"` row to `index.jsonl`, and structured metadata
with schema id `tracemux.pcap.packet.v1` to `frames.jsonl`. The metadata
includes sequence number, captured length, original length, link type,
interface id, raw offset, and raw length. `ts_origin` comes from the pcap packet
timestamp and `ts_ingest` comes from the tracemux server.

### FR-EXP-PCAPNG  pcapng export and direct writing
`tracemux export pcapng <session-dir> <dst>` and the authenticated server
export endpoint can render packet-shaped session-dirs as pcapng. The output
contains a Section Header Block, Interface Description Blocks for captured
link-type/interface combinations, and Enhanced Packet Blocks whose timestamps
derive from `ts_origin` and whose lengths preserve captured and original packet
lengths. Live pcap capture also supports direct pcapng output through
`save=pcapng` and concurrent session-dir plus pcapng output through
`save=both`.

### FR-DEC-PACKET-SUMMARY  Packet summary parser
The server can produce lightweight, bounded packet summaries for common
Ethernet/IP traffic without implementing full Wireshark-style dissection or TCP
stream reassembly. Summaries cover Ethernet II, VLAN ids, IPv4, IPv6, TCP, UDP,
ICMP, and ICMPv6 when present. Malformed or truncated packets return summary
errors instead of panicking; unsupported protocols preserve raw packet bytes and
use a generic protocol label when possible.

### FR-CLI-PCAP  CLI pcap source specs
CLI paths that accept source specs can parse URI-style pcap specs such as
`pcap://Ethernet?snaplen=65535&promisc=1&filter=tcp%20port%20502`. Parsed specs
round-trip through `ChannelSpec::Pcap` and render filesystem-safe kind/interface
tags. Without the native capture feature or required OS driver, opening a pcap
source fails with a clear public source-open error such as `E-1103` for backend
unavailable or `E-1101` for unclassified backend failures, and does not
register a partially opened session.

### FR-UI-PCAP  Packet capture UI
The web UI provides packet capture controls and packet views while preserving
server-owned persistence. Operators can select discovered interfaces, enter a
BPF filter, set snaplen and promiscuous mode, choose a UI publish mode, export
persisted pcap sessions as pcapng, and inspect a bounded packet list/detail/hex
view when packet publication is enabled. Browser packet state must remain
bounded and must not persist packet bytes except through explicit user-initiated
downloads.

### FR-MET-PCAP  Packet capture metrics
The server publishes packet capture metrics that can be rendered without
streaming raw packet data to the browser. Metrics include packet count, byte
count, kernel/backend drop count when available, application drop count,
capture queue depth, writer queue depth, packet rate, byte rate, and the last
packet-origin timestamp when known.

### NFR-PERF-PCAP  Medium-to-high rate packet capture
Packet capture separates persistence from UI fan-out so a slow browser cannot
block packet storage. High-rate captures should use BPF filters, snaplen, and
`publish=stats-only` or `publish=sampled`; full packet publication is an
operator opt-in. Browser packet lists are bounded, and overload must surface via
explicit counters rather than silent loss.

### NFR-REL-PCAP  Packet capture error handling
Missing native drivers, missing permissions, invalid interfaces, invalid BPF
filters, and pcapng writer failures are reported as public errors. Where the
backend provides enough signal, pcap startup failures use `E-1103` through
`E-1106` for backend unavailable, permission denied, invalid BPF filter, and
interface unavailable categories. Startup
failures before all required writers are ready must not register a live source
session. Completed or stopped captures leave durable artifacts readable for the
storage mode that was selected.

### NFR-SEC-PCAP  Packet capture security and privacy
Packet capture must not weaken existing authentication, server-owned
persistence, or privacy boundaries. UI clients never persist packet bytes
locally except through explicit browser downloads. HTTP export remains
authenticated unless loopback no-auth policy applies. Interface discovery is
kept minimal because interface names and addresses can leak host network
information, and pcap source specs must not embed secrets.

### NFR-PORT-PCAP  Packet capture portability
Windows x64 live capture is based on Npcap, Linux live capture is based on
libpcap, and macOS live capture is treated as best-effort until manually
validated. Normal CI and the default AI verification gate run without live
capture drivers by leaving `pcap-capture` disabled and using fake-backend tests.

### NFR-MAINT-PCAP  Packet capture dependency and review policy
Packet capture dependencies remain isolated and compatible with repository
policy: pcapng uses `pcap-file`, packet summaries use `etherparse`, and native
capture uses optional `pcap` behind `pcap-capture`. Driver/SDK-dependent checks
are explicit local/manual checks, `openssl-sys` remains banned, and critical
path changes such as `Cargo.lock`, core source traits, and protocol-facing
behavior require human review.
