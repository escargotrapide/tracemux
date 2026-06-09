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

## Windows shell (cmd / PowerShell)

The `process://` source spawns a child and captures its output, so it can run
an interactive Windows shell. Ready-to-use channels are in
[`examples/windows-shell.toml`](../../examples/windows-shell.toml).

```text
process:///cmd.exe?args=/K;chcp%2065001
process:///powershell.exe?args=-NoLogo;-NoProfile;-NoExit;-Command;[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
```

- `cmd /K <cmd>` runs `<cmd>` then keeps reading further commands from stdin.
  `powershell -NoExit -Command <cmd>` is the PowerShell equivalent.
- Reference the executable by name (`cmd.exe`, `powershell.exe`) so it is
  resolved on `PATH`. A full path must use backslashes
  (`C:\\Windows\\System32\\cmd.exe`); a forward-slash absolute path is
  mis-parsed by the Windows command interpreter.
- Keystrokes typed in the Terminal panel are sent back to the child stdin
  (`ProcessSink`), so the shell is usable as a line-oriented REPL.

### Pipe-mode limitations

This is a **pipe-based** source, not a pseudo-console (ConPTY). The child runs
with `isatty = false`, which means:

- No prompt repaint, no ANSI colours, and no cursor positioning.
- No terminal resize (rows/cols) propagation.
- Full-screen TUIs (`vim`, `less`, `fzf`, `more` paging) do not work.
- `Ctrl+C` and other console control events are not delivered reliably.
- `stdout` and `stderr` arrive as separate streams (stderr is tagged
  `kind = "stderr"`), not merged as a real console would.
- A pipe-mode child does not echo stdin and has no line discipline. The Terminal
  panel can run a local cooked mode (echo typed input, `Backspace` editing, send
  a whole line on Enter); this is controlled by the **Local echo** and **Line
  ending** selectors described below. Reopen the terminal after the source list
  updates so the input handler is attached.

A future ConPTY-backed source is required for a fully interactive terminal;
that change touches the frozen wire-protocol (a `resize` frame) and needs an
ADR plus human review.

### Terminal input: local echo and line ending

Local echo and the Enter line ending are configurable per source, because a
pipe-mode shell does not echo stdin and cmd.exe needs `CRLF` to run a line.

- **GUI**: the Terminal panel has **Local echo** (`auto` / `on` / `off`) and
  **Line ending** (`auto` / `CR` / `LF` / `CRLF`) selectors next to the encoding
  selector. With local echo on, typed input is shown locally, `Backspace` edits
  the line, and the whole line is sent with the chosen line ending on Enter.
- **CLI**: `tracemux send --newline cr|lf|crlf` appends the line ending to the
  payload (`crlf` for cmd.exe, `lf` for POSIX shells). Default is `none`.
- **Config**: a `[channels.<name>]` block accepts optional `local_echo`
  (`auto`/`on`/`off`) and `newline` (`auto`/`cr`/`lf`/`crlf`) keys.

`auto` resolves to a preset by source kind: `process` → echo on, `CRLF` (or
`LF` when the command looks like a POSIX shell); `serial` → echo off, `CR`;
`tcp`/`udp` → echo off, `LF`. The GUI applies these presets itself; reading the
config `local_echo`/`newline` into the browser requires a wire-protocol change
and is deferred (see the ConPTY note above).

### Encoding

The child console codepage and the channel decoding must agree:

- UTF-8: `chcp 65001` (cmd) or
  `[Console]::OutputEncoding=[System.Text.Encoding]::UTF8` (PowerShell), and
  keep the channel encoding at `utf-8`.
- Japanese (Shift_JIS): `chcp 932` and set the channel encoding to `shift_jis`
  in the Terminal panel's encoding selector.

### Security

Launching a shell is remote code execution by design. Keep `bind` on loopback
(`127.0.0.1` / `::1`) and require authentication on any routable interface. See
[`SECURITY.md`](../../SECURITY.md).

## Feature-gated or deferred specs

Some source kinds are intentionally registered before their full implementation
is available. They return `E-1101` with a clear message instead of registering
a partial source. Current deferred areas include MQTT, HTTP webhook, SSH,
Telnet, VISA, RTT, CAN, ETW, journald, and Windows Event Log.

## Secrets

Do not embed plaintext credentials in source specs. Use environment variables
or `secret://name` indirection where the source supports it.