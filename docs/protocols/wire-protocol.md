# Wire protocol — `tracemux.v1`

> **Frozen v0.1.** Changes require ADR + bumped subprotocol token
> (`tracemux.v2`) + a fixture compat test under `tests/compat/wire/`.

## Transport

- WebSocket Secure (WSS) over rustls.
- HTTP path: `/ws`. Subprotocol negotiation header:
  `Sec-WebSocket-Protocol: tracemux.v1, bearer.<token>`.
  Server accepts iff token validates with `argon2id`.
- Development loopback may use plain `ws://127.0.0.1:<port>/ws` with
  `tracemux serve --no-auth`; `--no-auth` is accepted only for
  loopback peers.
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

## `sub` / `unsub` payload

`sub` and `unsub` use envelope-level `sid` and optional `ch`. The
payload is currently an empty map. `sid` MUST be a UUID string and MUST
refer to a registered session. Unknown or malformed subscriptions return
a `ctl` error with `error_id = "E-2001"`.

```msgpack
{ type: "sub", sid: "uuid", ch: 0, payload: {} }
{ type: "unsub", sid: "uuid", ch: 0, payload: {} }
```

## `write` payload

`write` frames route client-provided bytes to the `Sink` paired with a
running source session. The envelope-level `sid` MUST be a UUID string
for a registered session. `ch` defaults to `0` when omitted. The
payload MUST contain `body` as a MessagePack bin value.

```msgpack
{
  type: "write",
  sid: "uuid",
  ch: 0,
  seq: 7,
  payload: {
    body: bin,
    target?: "host:port"  // UDP only; otherwise ignored
  }
}
```

Ordering is preserved per session by the server's per-sink write lock.
Successful writes return a `ctl` acknowledgement with the same `seq`:

```msgpack
{
  type: "ctl",
  sid: "uuid",
  ch: 0,
  seq: 7,
  payload: {
    event: "write_ack",
    sid: "uuid",
    ch: 0,
    bytes_written: 5,
    message: "write completed"
  }
}
```

Malformed writes, unknown `sid` values, source-only sessions, or stopped
sources return a `ctl` `error` event. Validation errors use `E-2001`;
transport-closed write failures use `E-1102`; generic sink failures use
`E-1001` unless a more specific core error is available.

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

## `ctl` payload

`ctl` is an extensible MessagePack map. Existing fields MUST be
preserved by v1 clients; unknown fields are ignored.

### Client actions

Client-to-server lifecycle requests use `payload.action`:

| Action    | Envelope fields | Payload fields | Effect |
| --------- | --------------- | -------------- | ------ |
| `list`    | none            | none           | Return a full source snapshot. |
| `start`   | none            | `spec` map, optional start overrides | Start a source from a `ChannelSpec`-compatible map. |
| `stop`    | `sid`           | none           | Abort a running source task but keep the session registered. |
| `resume`  | `sid`           | none           | Resume a stopped/completed spec-backed source with the same `sid`. |
| `restart` | `sid`           | optional start overrides | Abort if running and start the source again with the same `sid`. Supplied overrides update future resumes/restarts for that source. |
| `remove`  | `sid`           | none           | Stop the task and remove the session from the registry. |

`start.spec` is encoded as a map whose `kind` matches the source kind.
Implemented server-side v0.1 kinds are `serial`, `tcp`, `udp`, `file`,
`pipe`, `process`, `mock`, `replay`, `syslog`, `mqtt`, and
`http-webhook`. Other `ChannelSpec` variants are reserved until their
source implementation is wired into the server runner.

`start` MAY also include these backward-compatible optional fields as
siblings of `spec`. `restart` MAY include the same fields without
`spec`; omitted fields keep the source's previously stored options.

| Field | Type | Effect |
| ----- | ---- | ------ |
| `encoding` | string | Text encoding label for the source's decoder, e.g. `utf-8`, `shift_jis`, `cp932`. Unknown labels fall back to UTF-8 at the codec layer. |
| `classifier` | array | Classification rules. Each item is either `{ contains: string, tag: string, case_sensitive?: bool }` or `{ regex: string, tag: string, case_sensitive?: bool }`. Matching tags are added to decoded persisted records. Invalid regex rules are rejected. |
| `detection_mode` | string | Content detection mode: `configured`, `auto`, `suggest`, or `off`. `auto` may apply a high-confidence detected encoding; `suggest` reports candidates only. |
| `session_name_pattern` | string | Session-dir naming pattern for this start, using the same tokens as the server `--name-pattern` option. |

These fields override server defaults for this logical source lifetime
and are reused by `resume` / `restart`. On `restart`, supplied fields
become the new stored options for later lifecycle actions. Clients that
do not understand them can omit them; older servers ignore unknown `ctl`
fields.

Example:

```msgpack
{
  action: "start",
  spec: { kind: "file", path: "C:/logs/app.log", follow: true },
  encoding: "shift_jis",
  detection_mode: "suggest",
  classifier: [
    { contains: "ERROR", tag: "fault" },
    { regex: "E-[0-9]{4}", tag: "error-id" }
  ],
  session_name_pattern: "{prefix}_{kind}_{iface}_{unix_ns}"
}
```

Example restart that changes only the server-side text decoder from this
point forward while keeping the previous `classifier` and
`session_name_pattern`:

```msgpack
{
  action: "restart",
  encoding: "cp932"
}
```

### Server events

Server-to-client lifecycle acknowledgements also use `ctl` payloads:

| Event       | Fields | Meaning |
| ----------- | ------ | ------- |
| `sources`   | `sources` array | Full source table snapshot. |
| `started`   | `sid`, `message` | Source task registered and started. |
| `stopped`   | `sid`, `message` | Source task stopped. |
| `resumed`   | `sid`, `message` | Source resumed with the same `sid`. |
| `restarted` | `sid`, `message` | Source restarted with the same `sid`. |
| `removed`   | `sid`, `message` | Source removed from the registry. |
| `write_ack` | `sid`, `ch`, `bytes_written`, `message` | Write-back completed. |
| `error`     | `message`, `error_id` | Lifecycle or wire error. |

`sources` rows have this shape:

```msgpack
{
  sid: "uuid",
  name: "display name",
  kind: "mock",
  status: "running" | "stopped" | "unknown",
  channels: [0],
  bytes_in: 1234,
  persistent?: boolean,
  session_dir?: "tracemux-sessions/tracemux_serial_COM7_...",
  decoder?: "utf8-text:shift_jis",
  encoding?: "shift_jis",
  detection_mode?: "configured" | "auto" | "suggest" | "off",
  detection?: {
    mode: "auto",
    sample_bytes: 4096,
    configured_encoding: "utf-8",
    effective_encoding: "shift_jis",
    sampled_encoding: "shift_jis",
    encoding_candidates: [
      { label: "shift_jis", confidence: 96, had_errors: false, evidence: ["decode-clean"] }
    ],
    log_type_candidates: [
      { tag: "error-id", kind: "regex", pattern: "E-[0-9]{4}", count: 2, confidence: 85 }
    ]
  }
}
```

`persistent` and `session_dir` are advisory UI metadata. They describe
whether the server is writing a session-dir for the source and, if so,
the server-local path. `decoder` is the effective server-side decoder
label for this source lifetime when known. `encoding` is present only
for text decoders and names the character encoding portion clients may
use for live raw-byte display. `detection_mode` and `detection` are
advisory metadata from bounded server-side startup sampling. Clients MAY
display or accept suggestions by issuing a later `restart`, but MUST NOT
persist log bytes directly based on these fields; they are
display/navigation hints only.

Lifecycle wire/validation errors use `E-2001`; source-open failures use
`E-1101`.

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
