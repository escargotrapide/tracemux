# Virtual peer for development

`wanlogger-virt-peer` is a deterministic counterparty device used to verify
wanlogger sources without depending on real hardware. It lives in
`tools/virt-peer` and is intentionally a development/test tool rather than a
production data path.

## Modes

- `tcp` listens on or connects to a TCP endpoint. This is the default E2E test
  path because it needs no external driver.
- `serial` opens an existing COM port or Unix device path. On Windows, create a
  virtual COM pair with a driver such as com0com first; the tool does not create
  kernel devices by itself.

## Scripted traffic

The shared scenario layer can send text or hex payloads, repeat them, split
payloads into chunks, echo inbound bytes with an ACK prefix, and write a JSONL
transcript. `--initial-delay-ms` is useful when a test must subscribe to WSS
before the first device payload is emitted.

Useful options:

- `--send TEXT` adds a text payload.
- `--send-hex HEX` adds a binary payload such as `48656c6c6f0a`.
- `--eol lf|crlf|none` appends line endings to text payloads.
- `--repeat N` repeats the configured payload list.
- `--initial-delay-ms N` waits before the first scripted payload.
- `--interval-ms N` waits between scripted payloads.
- `--chunk-size N` splits outbound payloads.
- `--echo --ack-prefix ACK:` replies to inbound bytes.
- `--idle-timeout-ms N` exits after no inbound traffic for the given period.
- `--transcript PATH` writes JSONL rows for bytes and lifecycle events.

## TCP E2E path

The automated E2E test in `tools/virt-peer/tests/tcp_wss_e2e.rs` starts the
peer in TCP listen mode, starts the server WSS router in-process, issues a WSS
`ctl start` with a `tcp` source spec, subscribes to the returned session, and
asserts that the scripted peer bytes reach both WSS subscribers and the
server-created session-dir. It also exercises the HTTP session export route for
the same persisted source, so the smoke covers live delivery and read-back.

This path is the preferred smoke test before moving to virtual COM hardware
because it exercises the same server source runner and persistence path while
remaining driver-free.

## Browser plus driverless smoke

For local UI work on Windows, run the browser and virtual-peer checks without
installing COM or packet-capture drivers:

- `corepack pnpm --dir web e2e` starts Vite through Playwright and verifies the
  injected browser shell, connection-loss UI, export controls, and source note
  flows.
- `cargo test -p wanlogger-virt-peer --test tcp_wss_e2e` verifies the TCP peer,
  WSS source control, session-dir persistence, and HTTP export route.

These checks are the default driverless runtime gate. COM-pair and Npcap-backed
packet-capture validation remain manual or environment-gated follow-ups.

## AI verification workflow

AI agents can verify the virtual counterparty path without special hardware by
running the TCP E2E test or the aggregate repository gate:

- `cargo test -p wanlogger-virt-peer --test tcp_wss_e2e` verifies the virtual
  peer binary, server WSS control path, WSS subscription delivery, session-dir
  persistence, and peer transcript in one deterministic test.
- `just ai-verify` includes formatting, linting, tests, encoding checks, and RTM
  regeneration, and should be the final pre-review gate.

The TCP E2E path is intentionally driver-free. Serial/COM verification still
requires an existing real or virtual COM pair and should be treated as a manual
or environment-gated follow-up.

## Serial/COM path

For manual serial testing on Windows:

1. Install a virtual COM pair provider such as com0com.
2. Create a pair, for example `COM20` and `COM21`.
3. Start `wanlogger-virt-peer serial --port COM21 ...`.
4. Point wanlogger at `serial://COM20?baud=115200&data=8&parity=none&stop=1&flow=none`.

Serial tests that require a real or virtual COM device should stay ignored or
gated by environment variables so CI remains deterministic.
