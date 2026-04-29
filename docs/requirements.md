# Requirements

Each requirement has a stable id. Tests reference ids in comments
(`// REQ: FR-…`). `docs/rtm.md` is generated from those references.

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
`index.jsonl` file.

### FR-CLI-001  Import / export round-trip
The CLI guarantees that for any plain-text input file `F`,
`wanlogger import text F S` followed by
`wanlogger export text S G` produces a `G` whose final whitespace-
trimmed column for each row equals the corresponding line of `F` in
order.
