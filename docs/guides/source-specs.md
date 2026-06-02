# Source specs

Source specs use URI-style strings in the UI and CLI paths that accept source
specs. The browser parser and server parser both map these strings into a
`ChannelSpec`; the server remains the authority for opening and persisting the
source.

## Common specs

```text
mock://demo
file:///C:/logs/app.log?follow=1
tcp://127.0.0.1:5555
udp://0.0.0.0:5514
serial://COM3?baud=115200&data=8&parity=none&stop=1&flow=none
process:///C:/Windows/System32/cmd.exe?args=/c;ipconfig
pipe:///tmp/tracemux.sock
pcap://Ethernet?snaplen=65535&promisc=1&publish=stats-only
remote://wss%3A%2F%2Fedge.example.test%2Fws%3Fsid%3D00000000-0000-0000-0000-000000000000%26ch%3D0
```

## Packet capture options

`pcap://` accepts:

- `snaplen`: maximum captured bytes per packet.
- `promisc` or `promiscuous`: request promiscuous mode.
- `filter`: BPF filter, for example `tcp port 502`.
- `save`: `session`, `pcapng`, or `both`.
- `publish`: `stats-only`, `sampled`, or `full`.
- `pcapng`: direct pcapng output path when `save=pcapng` or `save=both`.

Use `publish=stats-only` for high-rate captures unless packet inspection in
the browser is required.

Packet capture opening failures use pcap-specific source error IDs when the
server can classify the cause: `E-1103` for backend unavailable, `E-1104` for
permission denied, `E-1105` for invalid BPF filters, and `E-1106` for missing
interfaces. Other source-open failures still use `E-1101`. Use detected
interface names exactly as reported and verify Npcap/libpcap installation plus
capture permissions before retrying.

## Feature-gated or deferred specs

Some source kinds are intentionally registered before their full implementation
is available. They return `E-1101` with a clear message instead of registering
a partial source. Current deferred areas include MQTT, HTTP webhook, SSH,
Telnet, VISA, RTT, CAN, ETW, journald, and Windows Event Log.

## Secrets

Do not embed plaintext credentials in source specs. Use environment variables
or `secret://name` indirection where the source supports it.