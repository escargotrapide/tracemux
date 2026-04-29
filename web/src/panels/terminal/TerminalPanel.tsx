// Terminal panel ? xterm.js + WebGL renderer (NFR-PERF-001).
// Subscribes to a (sid, ch), prints incoming bytes, and forwards user
// keystrokes back to the server via a `write` frame.
//
// REQ: FR-UI-002
// REQ: FR-UI-010
// REQ: FR-UI-011

import { createMemo, createSignal, For, onCleanup, onMount } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { sendWrite, sourcesStore, useChannel } from "~/state";
import { observeVisibility } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";

export interface TerminalPanelProps {
  sid: string;
  ch: number;
}

const encoder = new TextEncoder();

export function TerminalPanel(props: TerminalPanelProps) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;

  const [sid, setSid] = createSignal(props.sid);
  const [ch, setCh] = createSignal(props.ch);

  const sidOptions = createMemo(() => Object.values(sourcesStore));
  const chOptions = createMemo(() => {
    const s = sourcesStore[sid()];
    return s ? s.channels : [ch()];
  });

  function rebind(): void {
    unsub?.();
    unsub = useChannel(sid(), ch(), (p: DataPayload) => {
      if (p.body instanceof Uint8Array) {
        term?.write(p.body);
      } else if (typeof p.body === "object" && p.body) {
        term?.writeln(JSON.stringify(p.body));
      }
    });
  }

  onMount(() => {
    term = new Terminal({
      convertEol: true,
      cursorBlink: false,
      fontFamily:
        '"Cascadia Mono","Consolas","Hiragino Sans","Noto Sans Mono CJK JP",monospace',
      fontSize: 13,
      scrollback: 10_000,
      theme: { background: "#0e1116", foreground: "#c9d1d9" },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // CPU renderer fallback; spec allows this.
    }
    term.open(host);
    fit.fit();

    // TX: forward keystrokes to the server.
    term.onData((data) => {
      const bytes = encoder.encode(data);
      try {
        sendWrite(sid(), ch(), bytes);
      } catch {
        // ignore; surfaced via ctl error toast
      }
    });

    resizeObs = new ResizeObserver(() => fit?.fit());
    resizeObs.observe(host);

    rebind();
    unobserve = observeVisibility(host, { sid: sid(), ch: ch() });
  });

  onCleanup(() => {
    unobserve?.();
    unsub?.();
    resizeObs?.disconnect();
    term?.dispose();
  });

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        width: "100%",
        height: "100%",
      }}
    >
      <div
        style={{
          display: "flex",
          gap: "6px",
          padding: "4px 6px",
          "border-bottom": "1px solid var(--wl-border)",
          background: "var(--wl-bg-elev)",
        }}
      >
        <select
          value={sid()}
          onChange={(e) => {
            setSid(e.currentTarget.value);
            const opts = sourcesStore[e.currentTarget.value]?.channels ?? [];
            const first = opts[0];
            if (first !== undefined && !opts.includes(ch())) setCh(first);
            rebind();
          }}
          aria-label="sid"
        >
          <option value={sid()}>{sid()}</option>
          <For each={sidOptions().filter((s) => s.sid !== sid())}>
            {(s) => <option value={s.sid}>{s.name}</option>}
          </For>
        </select>
        <select
          value={ch()}
          onChange={(e) => {
            setCh(Number(e.currentTarget.value));
            rebind();
          }}
          aria-label="ch"
        >
          <For each={chOptions()}>
            {(c) => <option value={c}>ch {c}</option>}
          </For>
        </select>
      </div>
      <div ref={host!} style={{ flex: "1 1 auto", "min-height": 0 }} />
    </div>
  );
}
