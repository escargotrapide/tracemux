# Remote COM operation with human UI and AI clients

This runbook describes the architecture-safe path that works with the current
v0.1 implementation: the PC that owns the COM port runs `tracemux serve`, while
human UI clients and AI clients connect to that server over the existing
`tracemux.v1` wire protocol.

The key rule is simple: the server owns hardware, persistence, timestamps,
write-back routing, and audit. Browsers, Tauri, CLI tools, and AI agents are
clients only.

## Supported topology today

```text
COM device <-> COM-host PC running tracemux serve
                         |
                         | protected WS/WSS path
                         v
              browser / Tauri / CLI / AI client on another PC
```

Use this topology when a real or virtual COM port exists on one Windows host and
another PC needs to view the log or send commands. It keeps the existing
`Source -> Framer -> Decoder -> LogSink/UI` pipeline intact and avoids UI-side
or AI-side persistence.

## Current implementation status

Already available:

- Serial receive via `SerialSource`.
- Serial write-back via the paired `SerialSink` created by
  `SerialSource::open_duplex`.
- Server-side session-dir persistence under `--session-root`.
- Web UI source lifecycle actions, COM detection, terminal rendering, terminal
  keystroke TX, and an explicit send box.
- CLI write-back through `tracemux send`.
- CLI observation through `tracemux watch` JSONL output.
- `tracemux serve` bearer-token PHC loading via `--token-phc` and
  `--token-phc-file`.
- Optional HTTPS/WSS listener via `--tls` / `--tls-dir`.
- `remote` channel specs that mirror an edge `tracemux.v1` session into a
  central server-owned session and proxy write-back to the edge session.
- Write-back audit rows under the server session root.

Important caveats:

- `--no-auth` is accepted only for loopback peers. Do not expose a no-auth
  server directly on a LAN.
- The built-in TLS path can generate a self-signed certificate. Import the
  generated `server.crt` into the client trust store, or place a trusted reverse
  proxy in front of the loopback server. The server logs a TOFU fingerprint, but
  clients still need a trust path.
- Bearer token files contain PHC hashes only. Keep the plaintext token in an
  operator password manager, environment variable, or OS keyring; never commit
  it.
- A remote mirror subscribes to one known edge `(sid, ch)`. Discover the edge
  session with the UI source list or another `tracemux.v1` client before
  starting the mirror.

## COM-host setup

On the PC that physically owns the COM port:

1. Pick a session root on a local disk with enough space.
2. Start the server on loopback for local/Tauri use, or behind a protected
   tunnel for another PC.
3. Start the serial source from the UI or by autostarting selected ports.

For a direct LAN listener with TLS and bearer auth, generate a high-entropy
token outside the shell history, hash it, then start the server with the PHC
file:

```pwsh
$env:TRACEMUX_TOKEN = '<paste-generated-token>'
tracemux token-hash > C:\tracemux\edge-tokens.phc
Remove-Item Env:TRACEMUX_TOKEN

tracemux serve --bind 0.0.0.0:9000 --session-root C:\tracemux\edge-sessions --token-phc-file C:\tracemux\edge-tokens.phc --tls-dir C:\tracemux\tls --open-all-serial --serial-port COM3 --serial-baud 115200 --serial-data-bits 8 --serial-parity none --serial-stop-bits 1 --serial-flow none
```

If you use the generated self-signed certificate, copy
`C:\tracemux\tls\server.crt` to each client PC and trust it there, or use a
trusted HTTPS reverse proxy instead.

Example source spec for the UI Sources panel:

```text
serial://COM3?baud=115200&data=8&parity=none&stop=1&flow=none
```

For serial-heavy work, prefer explicit ports over "open every detected port" so
busy maintenance machines do not accidentally grab unrelated devices.

## Connecting from another PC

If you choose not to expose the built-in TLS/token listener directly, use one
of these protected paths instead of a raw no-auth LAN listener:

- SSH local port forwarding to the COM-host server.
- A VPN where host access is already authenticated and restricted.
- A reverse proxy that terminates HTTPS and enforces authentication before
  forwarding to a loopback-only `tracemux serve` instance.

Point the web UI at the forwarded endpoint with `VITE_TRACEMUX_URL`. When an
authenticated endpoint is available, also provide `VITE_TRACEMUX_TOKEN` so the
browser can send `bearer.<token>` in the WebSocket subprotocol and
`Authorization: Bearer ...` to HTTP helper APIs.

Loopback-only SSH tunnel example from the operator PC:

```pwsh
ssh -L 9000:127.0.0.1:9000 user@com-host
$env:TRACEMUX_TOKEN = '<edge-token>'
$env:VITE_TRACEMUX_URL = 'ws://127.0.0.1:9000/ws'
$env:VITE_TRACEMUX_TOKEN = $env:TRACEMUX_TOKEN
just dev-web
```

Direct WSS example after trusting the COM-host certificate:

```pwsh
$env:TRACEMUX_TOKEN = '<edge-token>'
$env:VITE_TRACEMUX_URL = 'wss://com-host.example.test:9000/ws'
$env:VITE_TRACEMUX_TOKEN = $env:TRACEMUX_TOKEN
just dev-web
```

Do not make `--no-auth` reachable from non-loopback addresses. The server code
rejects non-loopback no-auth WSS peers, but any proxy or firewall rule should
also enforce that boundary.

## Human workflow

1. Open the web UI or Tauri shell.
2. Use Sources -> detect serial ports, or paste a `serial://...` spec.
3. Start the checked port or explicit source spec.
4. Open the terminal panel for the returned session.
5. Observe incoming `data` frames in real time.
6. Send bytes from terminal keystrokes or the send box.
7. Export from the Sources detail pane when a portable artifact is needed.

The browser stores preferences, aliases, presets, and annotations only. Captured
log bytes stay in the server-owned session-dir.

## AI workflow

Treat the AI as a normal wire client, not as an in-process plugin or direct file
writer.

Recommended first rollout:

1. `ctl list` to discover active sessions.
2. `sub` to subscribe to the target `(sid, ch)`.
3. Decode MessagePack `data` frames and feed normalized records to the AI.
4. Have the AI propose actions first.
5. Send only approved bytes through the existing `write` frame path.

For write-back from automation that already knows the target session, use
`tracemux send` and wait for `write_ack`:

```pwsh
$env:TRACEMUX_TOKEN = '<edge-token>'
tracemux send --url wss://com-host.example.test:9000/ws --sid <edge-sid> --ch 0 --text 'status?' --wait-ack
```

For observation, use `tracemux watch` and consume its JSONL stream:

```pwsh
$env:TRACEMUX_TOKEN = '<edge-token>'
tracemux watch --url wss://com-host.example.test:9000/ws --sid <edge-sid> --ch 0
```

Polling `raw.bin` or `index.jsonl` from outside the server can bypass auth,
audit, and future retention policy.

## Central remote mirror

Use a central server when many COM-host PCs should be visible through one UI or
AI endpoint. The central server is a normal `tracemux serve` instance with its
own session root and client token. It connects outward to each edge server using
a `remote` channel spec.

On the central server host, keep the edge plaintext token in an environment
variable or secret store. The source spec stores only the indirection name:

```pwsh
$env:TRACEMUX_EDGE_TOKEN = '<edge-token>'
$env:TRACEMUX_TOKEN = '<central-token>'
tracemux token-hash > C:\tracemux-central\tokens.phc
Remove-Item Env:TRACEMUX_TOKEN

tracemux serve --bind 0.0.0.0:9100 --session-root C:\tracemux-central\sessions --token-phc-file C:\tracemux-central\tokens.phc --tls-dir C:\tracemux-central\tls
```

After the edge serial session is known, start a remote source from the central
UI using a percent-encoded edge WSS URL:

```pwsh
$edgeUrl = [uri]::EscapeDataString('wss://com-host.example.test:9000/ws?sid=<edge-sid>&ch=0&token_env=TRACEMUX_EDGE_TOKEN')
"remote://$edgeUrl"
```

The central source receives a new local `sid`. Human UI, `tracemux watch`, and
AI clients subscribe to that central `sid`. If they send a `write` frame to the
central remote session, the central server proxies the write to the edge
session and waits for the edge `write_ack`.

## Persistence and timestamps

Every source started by `tracemux serve` with a session root writes a
server-owned session-dir. For serial sessions, expect files such as:

- `meta.toml` for session metadata.
- `raw.bin` for lossless inbound bytes.
- `index.jsonl` for byte ranges and dual timestamps.
- `lines.jsonl` and `frames.jsonl` for decoded records.
- `audit.jsonl` at the session root for write-back/control audit events.

Every live `data` frame and persisted record carries both `ts_origin` and
`ts_ingest`, plus monotonic and clock-quality fields. Use these fields for
cross-PC analysis instead of replacing one timestamp with the other.

## Validation checklist

Before using real hardware, run a driver-free smoke test with the virtual peer
TCP path documented in `docs/dev/virtual-peer.md`. Then validate the serial path
with a real device or a virtual COM pair.

For a remote COM session, verify:

- The COM-host server creates a session-dir under the expected `--session-root`.
- The web UI receives data after subscribing to the serial session.
- Terminal send-box or keystroke TX returns `write_ack`.
- The device or virtual peer receives the outbound bytes.
- `index.jsonl` contains dual timestamp fields.
- `raw.bin` contains the original inbound bytes.
- `audit.jsonl` records write-back attempts.
- The server is not reachable without the intended tunnel, VPN, or proxy.
- If using a central mirror, the mirrored central `index.jsonl` preserves the
  edge `ts_origin` while recording a newer central `ts_ingest`.

## Future production hardening

For direct LAN or WAN exposure, keep these operational guardrails in place
before calling the deployment production-ready:

1. Rotate bearer tokens and PHC files using an operator-controlled process.
2. Trust the TLS certificate explicitly or use a certificate issued by an
  internal CA / reverse proxy.
3. Keep firewall rules narrow: expose only the intended WSS/HTTPS endpoint.
4. Monitor write-back audit rows and failed auth attempts.
5. Re-run the validation checklist after every certificate, token, or COM-port
  topology change.
