# Wire protocol — `wanlogger.v1`

> **Frozen v0.1.** Changes require ADR + bumped subprotocol token
> (`wanlogger.v2`) + a fixture compat test under `tests/compat/wire/`.

## Transport

- WebSocket Secure (WSS) over rustls.
- HTTP path: `/ws`. Subprotocol negotiation header:
  `Sec-WebSocket-Protocol: wanlogger.v1, bearer.<token>`.
  Server accepts iff token validates with `argon2id`.
- Future: WebTransport (HTTP/3) using identical frame format.
- `permessage-deflate` allowed only on text frames; binary frames
  carry MessagePack and are not re-compressed.

## Frame envelope

Every frame is a MessagePack map with the following fields:

| Field   | Type            | Notes                                     |
| ------- | --------------- | ----------------------------------------- |
| `type`  | string          | Frame type (see below)                    |
| `sid`   | string?         | Session id (UUID v4)                      |
| `ch`    | u32?            | Multiplex channel within the connection   |
| `seq`   | u64             | Monotonic per (connection, type)          |
| `payload` | any           | Type-specific                             |

## Frame types

| `type`            | Direction     | Purpose                              |
| ----------------- | ------------- | ------------------------------------ |
| `hello`           | client→server | client capabilities, app version     |
| `auth`            | client→server | bearer reauth (if not in subproto)   |
| `sub`             | client→server | subscribe to (sid, ch)               |
| `unsub`           | client→server | unsubscribe                          |
| `data`            | server→client | record envelope (see below)          |
| `ctl`             | both          | control event (connect, EOF, error…) |
| `write`           | client→server | write-back to a Sink                 |
| `metrics`         | server→client | server-side counters                 |
| `clientlog`       | client→server | UI logs forwarded to server logger   |
| `ping` / `pong`   | both          | RTT + clock sync                     |
| `clock_sync`      | both          | dedicated clock sync exchange        |
| `panel_priority`  | client→server | UI panel visibility / coalescing     |
| `child` (reserved)| —             | reserved for sub-mux                 |

## `data` payload

```msgpack
{
  ts_origin:        i64 (ns since UNIX epoch),
  ts_ingest:        i64 (ns since UNIX epoch),
  mono_ns:          u64,
  boot_id:          string,
  node_id:          string,
  clock_offset_ms:  i32,
  clock_quality:    "synced" | "best-effort" | "unknown" | "imported",
  drift_ppm:        f32,
  clock_source:     "system" | "ntp" | "ptp" | "monotonic" | "imported",
  sid:              string,
  ch:               u32,
  dir:              "in" | "out",
  kind:             "bytes" | "datagram" | "frame" | "record",
  body:             bin | map,
  level?:           "trace" | "debug" | "info" | "warn" | "error" | "fatal",
  tags?:            [string],
  correlation_id?:  string,
  source?:          string,
  host?:            string,
  schema_id?:       string,
}
```

## Limits (DoS hardening)

- At most 32 concurrent connections per server (configurable).
- At most 1 MiB per frame.
- At most 1 KiB/s sustained per connection without backpressure ack.
- Per-connection ring buffer default 8 MiB.

## Versioning

| Surface  | Version | Source of truth                                |
| -------- | ------- | ---------------------------------------------- |
| Subproto | `v1`    | `Sec-WebSocket-Protocol`                       |
| Schema   | `1.0.0` | this file                                      |
| Compat   | fixtures| `tests/compat/wire/v1/*`                       |
