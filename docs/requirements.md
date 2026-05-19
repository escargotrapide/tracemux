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
The server accepts WSS connections with subprotocol `wanlogger.v1`
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
The CLI / server is a single binary `wanlogger`. The Tauri app embeds
or sidecars the same binary.

<!-- New requirements: append below in numerical order. Do not renumber. -->

### FR-UI-001  Web shell
The web UI is a SolidJS application under `web/` that loads a Dockview
grid with `sources`, `metrics`, and `terminal` panels. It connects to
the server via WSS subprotocol `wanlogger.v1` and never persists log
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
`wanlogger serve` sidecar in production builds.

### FR-IMP-001  Plain-text importer
`wanlogger import text <src> <dst>` ingests a UTF-8 text file as one
record per `\n`-terminated line and produces a v0.1 session-dir at
`<dst>` with `raw.bin` + `index.jsonl`. Records carry
`clock_quality = imported`, `clock_source = imported`. The CLI refuses
to overwrite a non-empty destination directory.

### FR-EXP-001  Plain-text / CSV / JSONL exporters
`wanlogger export {text,csv,jsonl} <session-dir> <dst>` reads the
session-dir's `index.jsonl` + `raw.bin` and writes one row per record
to `<dst>`. The CLI refuses to run when `<session-dir>` lacks an
`index.jsonl` file. `--tz` formats exported timestamp fields in a
fixed display timezone such as `UTC`, `GMT+9`, `+09:00`, or
`Asia/Tokyo` without changing the stored session-dir.

### FR-CLI-001  Import / export round-trip
The CLI guarantees that for any plain-text input file `F`,
`wanlogger import text F S` followed by
`wanlogger export text S G` produces a `G` whose final whitespace-
trimmed column for each row equals the corresponding line of `F` in
order.

### FR-CLI-002  Wireshark extcap capture
`wanlogger extcap --capture --extcap-interface wanlogger --fifo PATH
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
`wanlogger send` connects to `wanlogger serve` using the
`wanlogger.v1` WSS subprotocol, sends bytes from `--text`, `--file`,
`--hex`, or stdin as a `write` frame, and optionally waits for
`write_ack` or `error` before exiting.

### FR-CLI-004  Send text encoding
`wanlogger send --text` accepts an `--encoding` option and encodes the
text payload with the selected character encoding before sending the
wire `write` frame. File, hex, and stdin payloads remain raw bytes.

### FR-CLI-005  Log classification tags
`wanlogger log` and `wanlogger serve` accept one or more substring
classification rules and store matching log-type tags in persisted
session-dir metadata. Matching is performed on decoded text and does
not alter the original raw bytes.

### FR-CLI-006  Serve text encoding
`wanlogger serve` accepts a default `--encoding` option for server-side
decoded text records. Captured raw bytes remain lossless; only decoded
`lines.jsonl` / `frames.jsonl` text and downstream classification use
the selected encoding.

### FR-CLI-007  Session-dir name patterns
`wanlogger log` and `wanlogger serve` accept a session-dir name pattern
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
and display timezone. These settings are persisted as UI preferences
and applied without storing log data in the browser.

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
