# wanlogger

> _That one unified terminal to view and maintain all the logs, either
> local or over networks._

A lightweight, high-functionality, multi-connection debug terminal and
log platform. Single Rust binary that runs as **CLI**, **server**, and
**Tauri sidecar**, plus a SolidJS web UI. Designed end-to-end for
AI-driven development with strong human-review guardrails on critical
paths.

## Status

**v0.1 — executable vertical slice in progress.** Trait surfaces are
frozen. The server can now start selected sources, route WSS
subscriptions, persist source output to a session-dir, and drive the
SolidJS source/terminal UI. Some source kinds and release-hardening
features remain stubbed or experimental; see
[`docs/structure.md`](docs/structure.md) for what is filled in.

## Highlights

- **Four-layer pipeline**: `Source → Framer → Decoder → LogSink/UI`
  plus orthogonal `Sink`, `Importer`, `Exporter`, `TimeseriesSink`,
  `TimeSource`. All trait surfaces are frozen at v0.1.
- **Server is the source of truth.** Browser, Tauri shell, and CLI all
  speak the same WSS wire protocol (`wanlogger.v1`, MessagePack).
- **Dual timestamps** on every record (`ts_origin` + `ts_ingest` plus
  `mono_ns`, `boot_id`, `node_id`, `clock_offset_ms`,
  `clock_quality`) — multi-PC log alignment is a first-class concern.
- **Secure-by-default**: rustls + `argon2id` bearer tokens + TOFU
  fingerprint pin + OS keyring secrets + `unsafe_code = "deny"`.
- **AI-maintainable**: `AGENTS.md`, `.github/skills/<task>/SKILL.md`,
  ADR + RTM, `human-review-required` label gate, `just ai-verify`.
- **Independent semver** for `wire-protocol`, `log-format`,
  `cli-output`, and `app`.

## Read this first

1. **[AGENTS.md](AGENTS.md)** — canonical map (build, layout,
   critical paths, pitfalls).
2. **[docs/architecture.md](docs/architecture.md)** — pipeline diagram.
3. **[docs/adr/0001-foundations.md](docs/adr/0001-foundations.md)** —
   the foundational decisions.
4. **[SECURITY.md](SECURITY.md)** — threat model and defaults.

## Quickstart (once toolchains are installed)

```bash
# Install Rust per rust-toolchain.toml and just
cargo install just
just build           # build the workspace
just test            # run tests
just ai-verify       # full gate (fmt + clippy + test + audit + deny + …)
```

For local UI development:

```bash
just dev-server      # loopback server at 127.0.0.1:9000, --no-auth
just dev-web         # SolidJS UI pointing at the loopback server
just dev-prepare     # build/copy the Tauri sidecar before desktop dev
just dev-tauri       # Tauri shell with bundled loopback sidecar
```

The web UI can start sources from URI-style specs such as
`mock://demo`, `file:///C:/logs/app.log?follow=1`, or
`tcp://127.0.0.1:5555`. Source presets are browser-local and store only
the source spec, never log data.

For serial-heavy sessions, the UI can detect COM ports and open the
checked ports in bulk. The server/CLI can also autostart serial ports at
launch with `wanlogger serve --open-all-serial`; add repeated
`--serial-port PORT` flags to restrict the set instead of opening every
detected port.

Server startup settings can be loaded from a TOML file with
`wanlogger serve --config wanlogger.toml`. The v1 config covers listener
settings, auth policy, TLS state, serial startup, export defaults,
live-delivery pacing, retention, and named startup channels; explicit
CLI flags such as `--bind`, `--no-auth`, and `--require-auth` override
overlapping config values.

```toml
config_version = 1

[server]
bind = "127.0.0.1:9443"
session_root = "wanlogger-sessions"
encoding = "utf-8"
detect_mode = "configured"
session_name_pattern = "{prefix}_{kind}_{iface}_{unix_ns}"
token_phc_files = ["tokens.phc"]
require_auth = false

[server.serial]
open_all = false
ports = ["COM3"]
baud = 115200
data_bits = 8
parity = "none"
stop_bits = 1
flow = "none"

[server.tls]
enabled = false
dir = "wanlogger-sessions/tls"

[export]
timezone = "UTC"
encoding = "utf-8"

[ui]
live_flush_ms = 0

[retention]
keep_days = 0

[channels.demo]
label = "demo source"
[channels.demo.spec]
kind = "mock"
tag = "demo"
```

Config files do not store plaintext bearer tokens. Use `token_phc_files`
with hashes produced by `wanlogger token-hash`, or pass token material
through the existing CLI/environment paths.
`wanlogger export --config wanlogger.toml` also reads the `[export]`
timezone and encoding defaults when `--tz` or `--encoding` is omitted.

For mixed encodings or reused source presets, `wanlogger serve` can run
bounded startup content detection with `--detect-mode auto`, `suggest`,
`configured`, or `off`. Auto mode may apply a high-confidence detected
text encoding to the server-side decoder; suggest mode leaves the
configured encoding active and exposes candidates in source metadata.
Substring rules from `--classify TEXT=TAG` and regular-expression rules
from `--classify-regex REGEX=TAG` are also evaluated against sampled
text so the UI can surface likely log-type tags before long captures.

Packet capture is available as an MVP. Driver-free builds include the pcap
source model, pcapng exporter, fake-backend tests, interface discovery schema,
metrics payloads, and bounded packet-list UI. Live Npcap/libpcap capture is
feature-gated behind `pcap-capture` so normal CI and development machines do
not need packet-capture SDKs or elevated capture privileges. See
[`docs/dev/packet-capture-live-setup.md`](docs/dev/packet-capture-live-setup.md)
for Windows/Npcap, Linux/libpcap, macOS, and direct pcapng setup notes.

The UI also includes browser-local ergonomics for daily log work:

- default, per-source, and per-channel display encodings for live byte
  decoding, with an explicit restart action to apply a selected
  per-source encoding to server-side decoded/persisted logs from that
  point forward;
- substring and regular-expression classification rules that surface as
  log-type/tag filters;
- source startup detection details, including suggested encodings and
  matching log-type rule candidates;
- source/session notes and log-type notes stored only as local
  annotations;
- timezone display controls accepting values such as `local`, `UTC`,
  `Asia/Tokyo`, `GMT+9`, or `+09:00`;
- export download filename patterns using `{sid}`, `{source}`,
  `{timestamp}`, `{format}`, and `{ext}` tokens.

Captured log bytes and session-dir persistence remain server-owned; the
browser stores preferences and annotations only.

For hardware-free source testing, use the virtual counterparty tool documented
in [`docs/dev/virtual-peer.md`](docs/dev/virtual-peer.md). Its TCP mode is the
driver-free E2E path; its serial mode works with an existing COM port or a
virtual COM pair.

For remote COM sessions where one PC owns the COM port and another PC, human UI,
or AI client needs to observe and send commands, see
[`docs/dev/remote-com-ai.md`](docs/dev/remote-com-ai.md). It describes the
current architecture-safe topology, protected connection options, persistence
checks, and the remaining hardening work before direct LAN exposure.

## License

Dual-licensed under MIT or Apache-2.0 at your option. See `LICENSE`.

