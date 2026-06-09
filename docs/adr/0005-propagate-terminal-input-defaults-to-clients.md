# ADR-0005: Propagating server-configured terminal-input defaults to clients

- **Status:** Accepted
- **Date:** 2026-06-09
- **Deciders:** tracemux maintainers
- **Related requirements:** FR-WIRE-001, FR-UI-002, FR-UI-011
- **Affected critical paths:** yes — `docs/adr/**`; if accepted, also
  `docs/protocols/wire-protocol.md`, `crates/server/src/wire.rs` (the
  `sources` ctl payload encoder in `crates/server/src/ws.rs`). Non-frozen
  follow-on: `crates/core/src/config/schema_v1.rs` consumers,
  `crates/server/src/source_manager.rs`, and the web sources store.

## Context

Phase 1 (commit `88c3841`) made the browser Terminal panel's input behaviour
configurable per source: **local echo** (`auto`/`on`/`off`) and the Enter
**line ending** (`auto`/`cr`/`lf`/`crlf`). These live client-side in
`web/src/state/terminalInput.ts` and are persisted in the browser. The same PR
also added **optional config keys** so an operator can declare defaults per
channel in `tracemux.toml`:

```toml
[channels.cmd]
local_echo = "on"
newline = "crlf"
[channels.cmd.spec]
kind = "process"
argv = ["cmd.exe", "/K", "chcp 65001"]
```

These keys are parsed today (`ChannelCfg.local_echo` / `ChannelCfg.newline` in
`crates/core/src/config/schema_v1.rs`) but are **not delivered to the browser**.
The reason is a wire-protocol gap: the browser learns about sources through the
`sources` control payload (`crates/server/src/ws.rs`, `list_sources_ctl`), whose
per-source row currently carries only:

```text
sid, name, kind, status, channels, bytes_in, persistent,
session_dir?, decoder?, encoding?, detection_mode?, detection?
```

There is **no field for terminal-input defaults** (nor a generic `tags`/`iface`
map) in that payload, and the web `SourceInfo`
(`web/src/state/index.ts`) has no corresponding field. So the GUI cannot read
the config-declared `local_echo`/`newline`; it falls back to its own
kind-based presets. This means a server operator's per-channel choice is
silently ignored by the UI.

The `sources` payload is part of the frozen `tracemux.v1` wire protocol
(`docs/protocols/wire-protocol.md`). Adding fields to it is a **frozen,
CRITICAL** change requiring this ADR, a version decision, and a compatibility
fixture (`tests/compat/wire/*`). ADR-0004 explicitly deferred this propagation
as out of scope; this ADR captures the decision separately.

Forces at play:

- **Operator intent:** config-declared defaults should reach the surface that
  honours them (the UI), not be parsed and dropped.
- **Compatibility:** the `sources` payload is frozen; old clients must keep
  working. The protocol's "unknown fields are ignored" rule
  (`docs/protocols/wire-protocol.md` ~line 135) allows additive change.
- **Authority:** the server is the source of truth; the browser only renders.
  Config defaults must flow server→client, but the user's explicit in-UI choice
  must still win over a server default.
- **Minimalism:** smallest additive change; keep `tracemux.v1` unchanged and
  bump only the app capability version.

## Decision

Add **two optional, additive fields** to each row of the `sources` control
payload, mirroring how `encoding`/`decoder`/`detection_mode` are already added
only when present:

```msgpack
// inside each element of payload.sources[]
{
  sid, name, kind, status, channels, bytes_in, persistent,
  session_dir?, decoder?, encoding?, detection_mode?, detection?,
  local_echo?: "auto" | "on" | "off",            // NEW, additive
  newline?:    "auto" | "cr" | "lf" | "crlf"      // NEW, additive
}
```

Semantics and rules:

1. Both fields are **optional** and emitted only when a configured default
   exists for that source (i.e. the channel's `ChannelCfg.local_echo` /
   `ChannelCfg.newline` were set). When absent, the client keeps its existing
   kind-based preset behaviour — so the untouched common case is byte-for-byte
   identical to today's payload.
2. The values are **server-declared defaults**, not commands. The browser uses
   them only as the initial value for a source the user has **not** yet
   overridden in the UI. An explicit in-UI choice (stored in
   `terminalInput.ts`) always wins. This preserves "server is truth for data,
   user controls their own view".
3. Additive and ignorable: old clients ignore the new fields; the `tracemux.v1`
   subprotocol string is **unchanged**. The **app capability version** is
   bumped.
4. Plumbing: `ChannelCfg.{local_echo,newline}` flow into the registered
   session's snapshot. Because `SessionState` currently stores only `label`,
   carry these as two new optional fields on `SourceStartOptions` →
   `SessionState` (or a small `terminal_defaults` struct), then surface them in
   `SourceSnapshot` and the `list_sources_ctl` encoder. The values are validated
   server-side against the allowed token sets; invalid tokens are dropped (not
   emitted) rather than failing source start.

## Consequences

- Positive:
  - Operator-declared `local_echo`/`newline` defaults reach the UI and seed the
    selectors, so a configured cmd/PowerShell channel "just works" without each
    user re-picking settings.
  - Additive and backward compatible; old clients ignore the fields and
    untouched sources are unchanged.
  - Reuses the existing "add field only when present" pattern in
    `list_sources_ctl`.
- Negative:
  - Touches a frozen critical path (`wire.rs` / `wire-protocol.md`), so it needs
    human review, an app-version bump, and a wire compatibility fixture.
  - Adds two optional fields to the most frequently sent control payload (only
    when configured), and threads two values through
    `SourceStartOptions`/`SessionState`/`SourceSnapshot`.
- Compatibility impact:
  - **wire:** additive optional `local_echo?`/`newline?` on `sources` rows;
    `tracemux.v1` subprotocol unchanged; app capability version bumped. New
    fixture under `tests/compat/wire/` proving (a) an old decoder ignores the
    fields and (b) a new decoder reads them.
  - **log:** none.
  - **cli:** none required; `tracemux watch`/source listing MAY surface the
    fields additively later.
  - **app:** the web sources store gains `localEchoDefault`/`newlineDefault`,
    and the Terminal panel uses them as the initial selector value when the user
    has no stored override.

## Alternatives considered

1. **Keep config keys server-only; never propagate** — rejected: it makes the
   config keys misleading (parsed but inert for the UI) and forces every user to
   re-pick what the operator already declared. (This is the accepted interim
   behaviour until this ADR is implemented.)
2. **Add a generic `tags: {k:v}` map to the `sources` payload** and carry
   `terminal.local_echo` / `terminal.newline` inside it — rejected for now: a
   generic tag bag is a larger, open-ended wire surface with its own naming and
   validation questions; two named optional fields are the minimal change. A
   generic tag map can be proposed separately if more per-source metadata needs
   to travel.
3. **Deliver defaults via a separate `ctl` event** (e.g. a one-shot
   `terminal_defaults` per source) — rejected: it races the `sources` sync the
   UI already consumes and needs its own correlation to a source row; folding
   the fields into the existing per-source row is simpler and atomic.
4. **Have the browser read `tracemux.toml` directly** — rejected: violates the
   server-is-truth boundary; the browser must not read server config files and
   may run on a different host.

## Migration plan

This changes the frozen `tracemux.v1` `sources` control payload (additively),
so:

1. Update [docs/protocols/wire-protocol.md](../protocols/wire-protocol.md) to
   document `local_echo?` / `newline?` on `sources` rows as optional additive
   fields emitted only when a configured default exists, with the allowed token
   sets.
2. Bump the **app** capability version (the `tracemux.v1` subprotocol string is
   unchanged because the change is additive and ignorable).
3. Add a `tests/compat/wire/` fixture demonstrating (a) a pre-change decoder
   ignores the fields and (b) a post-change decoder reads them.
4. Thread `local_echo`/`newline` from `ChannelCfg`
   (`crates/core/src/config/schema_v1.rs`) through `StartupChannel` →
   `SourceStartOptions` → `SessionState` → `SourceSnapshot`
   (`crates/server/src/source_manager.rs`), validating against the allowed
   tokens.
5. Emit the fields in `list_sources_ctl` (`crates/server/src/ws.rs`) only when
   present, mirroring `encoding`/`decoder`.
6. Add `localEchoDefault`/`newlineDefault` to `SourceInfo`/`SourceSyncPayload`
   (`web/src/state/index.ts`, `web/src/adapters/wss.ts`) and have
   `web/src/state/terminalInput.ts` use them as the initial value when no stored
   user override exists (user override still wins).
7. Regenerate `docs/rtm.md`.
8. Because this touches critical paths (`wire.rs`, `wire-protocol.md`), the
   implementing PR carries the `human-review-required` label and is **not**
   AI-self-merged.

### Relationship to ADR-0004

ADR-0004 (ConPTY + wire `resize`) and this ADR both add optional fields to the
frozen wire. They are independent: ADR-0004 changes the `write` payload for
terminal resize, while this ADR changes the `sources` control payload for
config-declared input defaults. If both are accepted, they SHOULD share a single
app-capability-version bump and a combined `tests/compat/wire/` update to avoid
two churns of the frozen surface.
