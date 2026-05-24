# Packet capture dependency investigation

This document records dependency candidates for the packet capture MVP described
in `docs/dev/packet-capture-mvp.md` and
`docs/dev/packet-capture-design.md`.

The repository policy constraints that matter most are:

- workspace Rust toolchain is 1.88.
- `unsafe_code = "deny"` applies to wanlogger code.
- `cargo deny` bans `openssl-sys` and only allows specific licenses.
- `Cargo.lock` is a critical path and requires human review when changed.

## Recommendation summary

| Purpose | Recommended choice | Why |
| --- | --- | --- |
| Live capture | `pcap = "2.4"` behind a feature | Mature libpcap/Npcap wrapper, MIT/Apache-2.0, Windows/Linux/macOS support. |
| pcapng write/export | `pcap-file = "2.0"` | Stable release, MIT, includes pcapng writer, avoids writing block encoding by hand. |
| Lightweight packet summary | `etherparse = "0.20"` | MIT/Apache-2.0, Rust 1.83, zero-allocation parsing path for Ethernet/IP/TCP/UDP. |
| Deterministic tests | In-tree fake backend | Avoids requiring Npcap/libpcap or live interfaces in normal CI. |

Avoid for the MVP:

- `pcap_on_demand`: attractive runtime loading idea, but old ecosystem
  dependencies such as futures 0.1 and mio 0.6 make it a poor default.
- `pcarp`: license is `Unlicense`, which is not allowed by current `deny.toml`.
- `pcap-parser`: useful parser, but not needed for writing pcapng and adds more
  parser dependencies than the MVP requires.
- `pcap-file-gsg`: MIT and newer RC fork, but prefer stable upstream
  `pcap-file = "2.0"` unless a required fix is missing.

## Live capture backend

### `pcap = "2.4"`

Observed metadata:

- Version: 2.4.0.
- License: `MIT OR Apache-2.0`.
- Rust version: 1.64.
- Purpose: packet capture API around libpcap/wpcap.
- Feature of interest: `capture-stream` for Tokio/futures integration.
- Platform support: libpcap on Linux/macOS, Npcap on Windows.

Pros:

- Best fit for Windows + Linux + optional macOS.
- Supports device listing, capture handles, BPF filters, datalink handling,
  buffer configuration, statistics, savefiles, and packet injection.
- License matches repository policy.
- Rust version is well below workspace 1.88.

Cons / operational requirements:

- Build and runtime require libpcap/Npcap availability.
- Windows builds require Npcap and usually the Npcap SDK library path.
- Linux builds require libpcap development headers.
- Capture privileges are environment-specific.
- Adding it updates `Cargo.lock`, which is a critical path.

Recommendation:

- Use `pcap = "2.4"` for the real backend.
- Feature-gate the real backend, for example `pcap-capture`, so normal CI can
  still run fake-backend tests without host packet-capture drivers.
- Keep an in-tree fake backend as the default test path.
- Do not enable `capture-stream` unless the implementation actually needs it;
  a blocking capture loop on a dedicated task/thread may be simpler and more
  predictable for backpressure.

### `pcap_on_demand = "0.1"`

Observed metadata:

- Version: 0.1.3.
- License: `MIT OR Apache-2.0`.
- Purpose: pcap/wpcap wrapper that loads libraries on demand.
- Dependencies include older ecosystem pieces such as futures 0.1, mio 0.6, and
  libloading 0.5.

Pros:

- Runtime loading can reduce link-time friction on machines without pcap
  installed.

Cons:

- Old dependency stack.
- Smaller and less common than `pcap`.
- More risk for long-term maintenance.

Recommendation:

- Do not use for the MVP.
- Revisit only if `pcap` linking becomes the main blocker for Windows packaging.

## pcapng writing/export

### `pcap-file = "2.0"`

Observed metadata:

- Version: 2.0.0.
- License: `MIT`.
- Purpose: read and write pcap and pcapng files.
- Includes `pcapng::PcapNgWriter` and pcapng block types such as section header,
  interface description, and enhanced packet blocks.
- Dependencies include `byteorder_slice`, `derive-into-owned`, and `thiserror`.

Pros:

- Stable release, not an RC.
- License is allowed by `deny.toml`.
- Provides pcapng writer primitives needed by the exporter.
- Avoids maintaining custom block padding, endian, and length encoding code.
- Uses `thiserror` 1.x, matching the current workspace major version.

Cons:

- Adds a new dependency tree.
- Need to confirm pcapng writer output against Wireshark and existing importer
  fixtures.

Recommendation:

- Use `pcap-file = "2.0"` for pcapng export and later direct streaming.
- Keep the wanlogger exporter responsible for mapping session-dir metadata to
  pcapng blocks.
- Add a synthetic export fixture and verify that Wireshark or the existing
  minimal importer can read the result.

### `pcap-file = "3.0.0-rc.2"` and `pcap-file-gsg = "3.0.0-rc4"`

Observed metadata:

- Both are MIT licensed.
- Both are release candidates.
- Both provide pcap/pcapng read/write support.

Recommendation:

- Avoid in MVP unless `pcap-file = "2.0"` lacks a required bug fix.
- Prefer stable dependencies for a new critical capture/export path.

### Custom pcapng writer

Pros:

- No new dependency for pcapng output.
- Full control over the small subset of pcapng needed by MVP.

Cons:

- Easy to get padding, endian, timestamp resolution, or block lengths wrong.
- More code to review and fuzz.
- Duplicates functionality available in `pcap-file`.

Recommendation:

- Do not write a custom writer first. Use `pcap-file = "2.0"` unless review
  finds a concrete blocker.

## Packet summary parser

### `etherparse = "0.20"`

Observed metadata:

- Version: 0.20.1.
- License: `MIT OR Apache-2.0`.
- Rust version: 1.83.
- Default feature: `std`.
- Supports Ethernet II, VLAN, ARP, IPv4, IPv6, UDP, TCP, ICMP, ICMPv6, and more.
- Provides zero-allocation slicing APIs for fast summaries.

Pros:

- Direct fit for the MVP packet summary and detail view.
- Rust version is compatible with workspace 1.88.
- License is allowed by `deny.toml`.
- Avoids heavier pnet macro stack.
- Can also generate synthetic packets for tests through `PacketBuilder`.

Cons:

- Not a full dissector stack.
- Industrial protocols still need explicit decoders later.

Recommendation:

- Use `etherparse = "0.20"` for L2-L4 summary parsing.
- Keep browser-side detail decode optional; server-side summary should remain
  bounded and cheap.

### `pnet_packet = "0.35"`

Observed metadata:

- Version: 0.35.0.
- License: `MIT OR Apache-2.0`.
- Purpose: packet parsing and manipulation.
- Dependencies include pnet macro crates.

Pros:

- Known packet manipulation ecosystem.
- License is allowed.

Cons:

- Heavier macro-oriented dependency stack for this use case.
- Less attractive than `etherparse` for a lightweight summary-only MVP.

Recommendation:

- Do not use initially. Revisit only if `etherparse` lacks specific protocol
  support needed by the first packet details.

## pcap/pcapng parsing candidates

### `pcap-parser = "0.17"`

Observed metadata:

- Version: 0.17.0.
- License metadata: `MIT/Apache-2.0`.
- Rust version: 1.65.
- Parser-focused crate using dependencies such as `nom`.

Pros:

- Useful if a robust pcap/pcapng importer becomes the priority.
- Has serialization feature hooks.

Cons:

- MVP primarily needs pcapng writing, not parsing.
- Existing repository already has a minimal pcapng importer.
- More dependency surface than `pcap-file` for the exporter path.

Recommendation:

- Do not add for the MVP.

### `pcarp = "2.0"`

Observed metadata:

- Version: 2.0.0.
- License: `Unlicense`.
- Purpose: pure Rust pcapng reading.

Recommendation:

- Do not use. Current `deny.toml` does not allow `Unlicense`.

## Proposed Cargo feature shape

Implemented split:

- `pcap-file` and `etherparse` are normal dependencies because pcapng export
  and packet summaries are driver-free, pure Rust, and part of the default MVP.
- `pcap-capture`: enables optional `pcap` and the real Npcap/libpcap backend.

The fake backend and synthetic pcapng exporter tests should work without live
capture hardware. The real backend tests should be manual or environment-gated.

The default `just ai-verify` gate intentionally excludes `pcap-capture`; run
feature-enabled checks on hosts with Npcap/libpcap configured.

## Platform setup implications

Windows:

- Install Npcap.
- Install the Npcap SDK for builds that link through `pcap`.
- Ensure the SDK `Lib` or `Lib/x64` path is visible through `LIB`.
- Verify packaging for Tauri/CLI so missing Npcap produces a clear error.

Linux:

- Install libpcap development package for builds.
- Grant capture privileges through root or capabilities.
- Manual tests should verify BPF filters and drop counters.

macOS:

- libpcap is usually available.
- Use a non-zero capture timeout to avoid blocking behavior.
- Treat support as best-effort until manually validated.

## Risk assessment

| Risk | Mitigation |
| --- | --- |
| `pcap` makes normal builds require Npcap/libpcap. | Feature-gate real backend and keep fake backend default for CI. |
| Windows SDK setup is fragile. | Document setup and surface `E-1101` style source-open errors clearly. |
| pcapng output compatibility issues. | Use `pcap-file = "2.0"`, add synthetic fixtures, test with importer/Wireshark. |
| Too much dependency churn. | Add only `pcap-file` and `etherparse` first; add `pcap` when real backend starts. |
| License/source policy failures. | Avoid `pcarp`; run `cargo deny` after dependency changes. |
| UI overload from high packet rates. | Keep dependency choices separate from publish policy; do not stream all packets to UI by default. |

## Final recommendation status

The MVP implementation follows the recommendation:

1. `pcap-file = "2.0"` is used for pcapng export and direct streaming.
2. `etherparse = "0.20"` is used for packet summaries.
3. Fake pcap backend and all session/export/UI paths work without requiring
   live capture drivers.
4. `pcap = "2.4"` is optional behind `pcap-capture` for the real backend.
5. Run `cargo deny`, clippy, tests, and `just ai-verify` after dependency and
   implementation changes.
