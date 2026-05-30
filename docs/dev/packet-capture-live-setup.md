# Packet capture live backend setup

This note covers the optional `pcap-capture` feature added for live Npcap/libpcap
packet capture. Default wanlogger builds keep this feature disabled so normal CI
and development machines do not need packet-capture drivers, SDKs, or elevated
capture privileges.

## Feature shape

Enable the native backend with:

- `wanlogger-core/pcap-capture` for library builds.
- `wanlogger-server/pcap-capture` for server-only builds.
- `wanlogger-cli/pcap-capture` for the `wanlogger` binary.

Example package build:

```pwsh
cargo build -p wanlogger-cli --features pcap-capture
```

When the feature is disabled, opening a `pcap://...` source fails with `E-1101`
and a message that the backend is not available in this build.

The default `just ai-verify` gate intentionally excludes `pcap-capture` because
it links host-native packet-capture libraries. On machines with Npcap/libpcap
configured, run an explicit feature check in addition to the default gate:

```pwsh
cargo check -p wanlogger-cli --features pcap-capture
cargo clippy -p wanlogger-core --features pcap-capture --lib -- -D warnings
```

## Windows / Npcap

1. Install Npcap. Enable WinPcap API compatibility only if your local policy
   requires it.
2. Install the Npcap SDK for builds that link the Rust `pcap` crate.
3. Make the SDK library directory visible to the linker, for example by adding
   the SDK `Lib` or `Lib/x64` directory to `LIB` in the build shell.
4. Build with `--features pcap-capture`.
5. Run wanlogger from an account allowed to capture on the selected adapter.

Manual smoke test:

```pwsh
$env:WANLOGGER_PCAP_TEST_IFACE = "<Npcap device name>"
cargo test -p wanlogger-core --features pcap-capture -- --ignored native_backend_can_open_env_iface
```

Use `/api/detect` from a feature-enabled server to discover candidate device
names. The detect payload intentionally omits interface addresses until the
security policy for authenticated discovery is finalized.

## Linux / libpcap

1. Install libpcap development headers, for example `libpcap-dev` on Debian or
   Ubuntu distributions.
2. Grant capture privileges through root, capabilities, or a distro-approved
   packet-capture group.
3. Build with `--features pcap-capture`.

Manual smoke test:

```sh
WANLOGGER_PCAP_TEST_IFACE=eth0 cargo test -p wanlogger-core --features pcap-capture -- --ignored native_backend_can_open_env_iface
```

## macOS / libpcap

macOS normally ships libpcap. Build with `--features pcap-capture` and run from
an account with permission to capture on the selected interface. Treat macOS as
best-effort until a manual smoke test confirms the selected interface, timeout,
and BPF filter behavior.

## Operational notes

- `snaplen`, promiscuous mode, timeout, buffer size, and BPF filter are applied
  before packet publication.
- `save=session` persists the normal wanlogger session-dir only.
- `save=pcapng` writes a direct pcapng artifact only. `save=both` writes the
   session-dir and direct pcapng artifact together.
- If `pcapng_path` is omitted and a server session-dir exists, direct pcapng
   output defaults to `capture.pcapng` inside that session-dir. Rotated parts use
   `capture.0001.pcapng`, `capture.0002.pcapng`, and so on.
- If `save=pcapng` is requested without either `pcapng_path` or a server
   session-dir root, startup fails before the source session is registered.
- Direct pcapng writer creation and append failures fail the pcap runner. In
   `save=both` mode this prevents silent divergence between the session-dir and
   the direct artifact; operators should treat any partial pcapng as suspect and
   export from the session-dir when available.
- UI downloads for session exports are native browser downloads backed by the
   server export API. Large pcapng and multi-source ZIP downloads should be
   requested from the UI buttons instead of by copying browser-fetched blobs.
- Bulk "Export all sources" ZIP downloads are assembled on the server through a
   short-lived one-use download ticket, which avoids loading all source artifacts
   into browser memory.
- Captured packet timestamps are interpreted as libpcap's default microsecond
  `timeval` values and converted to Unix nanoseconds for `ts_origin`.
- Kernel/backend drops are surfaced through pcap statistics when the backend
  provides them.
- UI packet publication is still controlled by `publish=stats-only|sampled|full`;
  high-rate captures should not use `full` unless the operator accepts the UI
  load.
- The default AI gate validates the driver-free path. Real hardware validation
   remains manual until CI runners with Npcap/libpcap privileges are available.
