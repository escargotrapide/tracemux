# ADR-0004: ConPTY/PTY interactive terminal source and wire resize signalling

- **Status:** Accepted
- **Date:** 2026-06-09
- **Deciders:** tracemux maintainers
- **Related requirements:** FR-SRC-PROCESS, FR-SINK-PROCESS, FR-WIRE-001,
  FR-UI-002, FR-UI-011 (a new FR-SRC-PTY is proposed below)
- **Affected critical paths:** yes — `docs/adr/**`; if accepted, also
  `docs/protocols/wire-protocol.md`, `crates/server/src/wire.rs`,
  `crates/core/src/source/mod.rs` (only if a resize hook is added to the
  trait — see Alternatives), and new non-frozen files under
  `crates/core/src/source/` and the web terminal panel.

## Context

The `process://` source (`crates/core/src/source/process.rs`) spawns a child
with piped `stdin`/`stdout`/`stderr`. Because the child's standard streams are
pipes, the child sees `isatty = false` and runs in a non-interactive mode. This
is fine for line-oriented command output and a basic REPL, but it cannot
provide a *real* interactive terminal:

- No prompt repaint, line editing, ANSI colours, or cursor positioning.
- No terminal size: full-screen TUIs (`vim`, `less`, `fzf`, `more` paging,
  PowerShell PSReadLine editing) render incorrectly or refuse to run.
- `Ctrl+C` and other console control events are not delivered reliably.
- `stdout` and `stderr` are two separate streams, not one merged VT stream.
- The child does not echo stdin (no line discipline).

Phase 1 (commit `88c3841`) added a **client-side workaround**: the web Terminal
panel offers per-source local echo and a configurable Enter line ending
(`web/src/state/terminalInput.ts`), and the CLI gained `send --newline`. That
makes cmd.exe and PowerShell usable as line REPLs, but it cannot deliver colour,
resizing, or TUIs, because those require the OS to present a real terminal to
the child.

A **pseudo-console** does exactly that: Windows **ConPTY**
(`CreatePseudoConsole`) and Unix `openpty(3)`/`forkpty(3)` allocate a PTY pair
so the child believes it is attached to a terminal. The cross-platform
`portable-pty` crate (used by WezTerm) wraps both. With a PTY, the child emits a
single VT byte stream (colour, cursor control, screen clears) and honours its
window size.

The missing piece is **resize signalling end-to-end**. A terminal has a size
(`cols` × `rows`). The browser's `xterm.js` changes size as its panel/window
resizes; that new size must reach the PTY so the child (e.g. `vim`) can repaint
correctly:

```
xterm.js (80x24 -> 120x40)
   │  needs a "resize{cols, rows}" message  (does NOT exist in tracemux.v1 today)
   ▼
server -> Pty.resize(cols, rows) -> child SIGWINCH / ConPTY ResizePseudoConsole
```

The wire protocol (`docs/protocols/wire-protocol.md`, subprotocol
`tracemux.v1`) currently has no frame or payload field for terminal resize. The
`write` payload carries only `body` (bin) and an optional UDP `target`. Adding
resize is therefore a **frozen, CRITICAL wire change** that requires this ADR, a
version decision, and a compatibility fixture (`tests/compat/wire/*`).

Forces at play:

- **Compatibility:** `tracemux.v1` is frozen; existing clients must keep
  working. The protocol already states "unknown fields are ignored"
  (`docs/protocols/wire-protocol.md` line ~135), which enables additive change.
- **Security:** a PTY shell is full remote code execution, same as the existing
  process source. Resize must be bounded (sane `cols`/`rows` caps) to avoid
  abuse.
- **Minimalism:** prefer the smallest additive change that the
  forward-compatibility rule already permits, and keep the `tracemux.v1`
  subprotocol string unchanged (bump only the app capability version).
- **Cross-platform parity:** the same source abstraction must cover Windows
  (ConPTY) and Unix (openpty).

## Decision

Introduce a PTY-backed interactive terminal as a **new, additive** source kind
and add **one additive wire field** for resize. Nothing existing changes shape.

### 1. New `pty` source kind (non-frozen code, additive enum variant)

Add a `PtySource` (and paired `PtySink` for stdin) under
`crates/core/src/source/pty.rs`, backed by `portable-pty` so a single
implementation covers ConPTY and openpty. It spawns the child attached to a PTY
slave, reads the merged VT stream as `Frame::Bytes`, and exposes a resize hook.

Add an additive `ChannelSpec::Pty` variant (the enum is `#[non_exhaustive]`, so
this is additive) carrying `argv`, optional initial `cols`/`rows`, and optional
`cwd`/`env`. URI form: `pty:///cmd.exe?args=/K;...&cols=120&rows=40`, plus a
`process://...?pty=1` alias that maps to `ChannelSpec::Pty`.

A new requirement **FR-SRC-PTY** documents this source; a new error id
**E-1107 (`E1107PtyUnavailable`)** reports PTY allocation failures (the next
free id after the pcap range `E-1103..E-1106`).

### 2. Additive wire resize signalling

Carry resize as an **optional, additive field on the existing `write`
payload**, reusing the already-routed `(sid, ch)` plumbing rather than adding a
new frame type:

```msgpack
{
  type: "write",
  sid: "uuid",
  ch: 0,
  seq: 7,
  payload: {
    body?:  bin,             // now OPTIONAL: a resize-only frame omits body
    target?: "host:port",
    resize?: { cols: u16, rows: u16 }   // NEW, additive, optional
  }
}
```

Semantics and rules:

1. `resize` is **optional**. When present, the server clamps `cols`/`rows` to a
   sane range (e.g. `1..=10000`) and calls the PTY resize hook for the target
   session. Non-PTY sinks ignore `resize`.
2. `body` becomes **optional** in the `write` payload so a client can send a
   resize-only frame. A `write` with neither `body` nor `resize` is a `E-2001`
   validation error. This is backward compatible: every existing client always
   sends `body`, and a server that predates this ADR already ignores unknown
   fields, so it treats a resize-only frame as an empty/invalid write — which is
   acceptable because only new (PTY-aware) clients send resize-only frames.
3. The `tracemux.v1` subprotocol string is **unchanged** (the change is additive
   and ignorable). The **app capability version** is bumped, and the `hello`
   capabilities advertise `pty` so a UI only offers a real terminal when the
   server supports it.
4. Resize is acknowledged by reusing the existing `write_ack`/`ctl error` path
   with the same `seq` (no new ctl event needed); `bytes_written = 0` for a
   resize-only frame.

### 3. UI

The web Terminal panel, when the selected source is a `pty` kind, switches to
raw pass-through (no local cooked mode — the PTY echoes), forwards `xterm`
bytes verbatim, and sends a `resize` `write` frame from the `FitAddon` on size
changes (debounced). The Phase 1 local-echo/line-ending controls are hidden for
`pty` sources (the PTY provides echo and line discipline).

## Consequences

- Positive:
  - A real interactive terminal: colour, cursor control, full-screen TUIs,
    PSReadLine/readline editing, and correct resizing for cmd, PowerShell,
    bash, etc.
  - One cross-platform code path (ConPTY + openpty via `portable-pty`).
  - Additive and backward compatible: old clients ignore `resize`; existing
    `write` frames (always carrying `body`) are byte-for-byte unchanged.
  - Reuses `(sid, ch)` routing and the `write_ack` path; no new frame type.
- Negative:
  - Touches a frozen critical path (`wire.rs`, `wire-protocol.md`), so it needs
    human review, an app-version bump, and a wire compatibility fixture.
  - New dependency `portable-pty`; must pass `cargo deny` (no `openssl-sys`).
  - PTY shells are full RCE; loopback/auth posture and `cols`/`rows` clamping
    must be enforced. `SECURITY.md` needs a note.
  - Making `body` optional slightly loosens `write` validation; mitigated by
    rejecting frames that carry neither `body` nor `resize`.
- Compatibility impact:
  - **wire:** additive optional `resize` field and `body` made optional;
    `tracemux.v1` subprotocol unchanged; app capability version bumped. New
    fixture under `tests/compat/wire/` proving (a) an old decoder ignores
    `resize` and (b) a new decoder reads it, plus a resize-only frame round
    trip.
  - **log:** none. PTY output persists as ordinary `bytes` frames; VT escapes
    are part of the byte stream (renderers/exporters already handle raw bytes).
  - **cli:** additive only — a future `tracemux shell` (interactive raw-mode
    attach) MAY be added later; not required by this ADR.
  - **app:** UI gains a real terminal mode for `pty` sources and resize
    forwarding.

## Alternatives considered

1. **A dedicated new `resize` frame type** (e.g. `type: "resize"`) — rejected
   for now: it is a larger surface (new top-level frame, new handler, new
   fixture family) than reusing the already-routed `write` payload with an
   additive field. The additive-field approach matches the precedent in
   ADR-0003.
2. **Add a `resize()` method to the frozen `Source`/`Sink` trait** — rejected:
   `crates/core/src/source/mod.rs` and `sink/mod.rs` are frozen critical traits.
   Instead, expose resize via a narrow, source-specific channel (e.g. an
   `mpsc` the `SourceManager` holds for PTY sessions, or a small dedicated
   `Resizable` capability trait that `PtySource` implements) so the frozen trait
   surface is untouched.
3. **Carry resize inside a `ctl` event from the client** — rejected: `ctl` is
   used for control/acknowledgement events and adding client→server control
   commands there blurs its role; the `write` channel already carries
   client→sink intent with `(sid, ch)` and `seq` acknowledgement.
4. **Retrofit ConPTY onto the existing `process://` source** (no new kind) —
   rejected: it would change the semantics of an existing stable source
   (separate stdout/stderr, no echo) into a merged VT stream, which is a
   behavioural break for current process consumers. A new `pty` kind keeps both
   behaviours available.
5. **Keep only the Phase 1 client-side workaround** — rejected as the long-term
   answer: local echo + line ending cannot deliver colour, resizing, or TUIs.
   It remains the accepted behaviour for the plain `process://` source.

## Migration plan

This changes the frozen `tracemux.v1` wire `write` payload (additively), so:

1. Update [docs/protocols/wire-protocol.md](../protocols/wire-protocol.md):
   document `resize?: { cols, rows }` and that `body` is optional when `resize`
   is present, with the "reject frames with neither `body` nor `resize`" rule
   and the `cols`/`rows` clamp range.
2. Bump the **app** capability version and advertise a `pty` capability in
   `hello`; the `tracemux.v1` subprotocol string is unchanged.
3. Add a `tests/compat/wire/` fixture set: (a) a pre-change decoder ignores
   `resize`, (b) a post-change decoder reads it, (c) a resize-only `write` frame
   round trips.
4. Add `portable-pty` to `crates/core` and confirm `cargo deny` passes
   (no `openssl-sys`, license allowed).
5. Implement `PtySource`/`PtySink` (`crates/core/src/source/pty.rs`,
   `crates/core/src/sink/pty.rs`), the `ChannelSpec::Pty` variant + URI parsing
   (`crates/cli/src/cmd/spec.rs`, `web/src/state/sourceSpec.ts`), and the
   `SourceManager` wiring (`crates/server/src/source_manager.rs`) including the
   resize hook and `cols`/`rows` clamping.
6. Add `FR-SRC-PTY` to `docs/requirements.md` and error id
   `E-1107 PtyUnavailable` to `crates/core/src/error_id.rs` +
   `docs/errors/E-1107.md`; regenerate `docs/rtm.md`.
7. Update the web Terminal panel to raw mode + resize forwarding for `pty`
   sources, hiding the Phase 1 local-echo/line-ending controls there.
8. Add a `SECURITY.md` note that PTY sources are RCE and must stay on
   loopback/authenticated interfaces.
9. Because this touches critical paths (`wire.rs`, `wire-protocol.md`,
   potentially trait files), the implementing PR carries the
   `human-review-required` label and is **not** AI-self-merged.

### Out of scope (separate follow-up)

- `config → GUI` propagation of the Phase 1 `local_echo`/`newline` defaults
  (`ChannelCfg`) is a different additive wire change (source-sync payload) and
  is tracked separately; it is not required for ConPTY and is not decided here.
