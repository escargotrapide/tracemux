// Terminal panel ? xterm.js with WebGL renderer (NFR-PERF-001).
// Subscribes to a single (sid, ch) and prints incoming bytes.

import { onCleanup, onMount } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { useChannel } from "~/state";
import { observeVisibility } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";

export interface TerminalPanelProps {
  sid: string;
  ch: number;
}

export function TerminalPanel(props: TerminalPanelProps) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;

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

    resizeObs = new ResizeObserver(() => fit?.fit());
    resizeObs.observe(host);

    unsub = useChannel(props.sid, props.ch, (p: DataPayload) => {
      if (p.body instanceof Uint8Array) {
        term?.write(p.body);
      } else if (typeof p.body === "object" && p.body) {
        term?.writeln(JSON.stringify(p.body));
      }
    });

    unobserve = observeVisibility(host, { sid: props.sid, ch: props.ch });
  });

  onCleanup(() => {
    unobserve?.();
    unsub?.();
    resizeObs?.disconnect();
    term?.dispose();
  });

  return <div ref={host!} style={{ width: "100%", height: "100%" }} />;
}
