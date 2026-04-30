// Terminal panel: xterm.js + WebGL renderer (NFR-PERF-001).
// Subscribes to a (sid, ch), prints incoming bytes, and forwards user
// keystrokes back to the server via a `write` frame.
//
// REQ: FR-UI-002
// REQ: FR-UI-010
// REQ: FR-UI-011

import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
} from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { t } from "~/i18n";
import {
  selectTerminalChannel,
  sendWrite,
  sourcesStore,
  terminalChannel,
  useChannel,
} from "~/state";
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
  const hasActiveSource = createMemo(() => Boolean(sourcesStore[sid()]));

  function rebind(): void {
    unsub?.();
    unsub = null;
    if (!hasActiveSource()) return;
    unsub = useChannel(sid(), ch(), (p: DataPayload) => {
      if (p.body instanceof Uint8Array) {
        term?.write(p.body);
      } else if (typeof p.body === "object" && p.body) {
        term?.writeln(JSON.stringify(p.body));
      }
    });
  }

  function reobserve(): void {
    unobserve?.();
    unobserve = null;
    if (host && hasActiveSource()) {
      unobserve = observeVisibility(host, { sid: sid(), ch: ch() });
    }
  }

  function bind(nextSid: string, nextCh: number): void {
    if (nextSid === sid() && nextCh === ch()) return;
    setSid(nextSid);
    setCh(nextCh);
    rebind();
    reobserve();
  }

  createEffect(() => {
    const selected = terminalChannel();
    if (!selected) return;
    bind(selected.sid, selected.ch);
  });

  createEffect(() => {
    if (hasActiveSource()) return;
    const first = sidOptions()[0];
    if (!first) return;
    selectTerminalChannel(first.sid, first.channels[0] ?? 0);
  });

  function clearTerminal(): void {
    term?.clear();
  }

  function copySelection(): void {
    const text = term?.getSelection() ?? "";
    if (!text) return;
    void navigator.clipboard?.writeText(text);
  }

  function safeFit(): void {
    try {
      fit?.fit();
    } catch {
      // Dockview may still be settling panel dimensions; the next
      // ResizeObserver tick will retry. Keep the UI smoke-test quiet.
    }
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
    requestAnimationFrame(safeFit);

    // TX: forward keystrokes to the server.
    term.onData((data) => {
      if (!hasActiveSource()) return;
      const bytes = encoder.encode(data);
      try {
        sendWrite(sid(), ch(), bytes);
      } catch {
        // ignore; surfaced via ctl error toast
      }
    });

    resizeObs = new ResizeObserver(() => requestAnimationFrame(safeFit));
    resizeObs.observe(host);

    const selected = terminalChannel();
    if (selected) {
      setSid(selected.sid);
      setCh(selected.ch);
    }
    rebind();
    reobserve();
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
            const nextSid = e.currentTarget.value;
            if (!nextSid) {
              bind("", 0);
              return;
            }
            const opts = sourcesStore[nextSid]?.channels ?? [];
            const first = opts[0];
            const nextCh = first !== undefined && !opts.includes(ch()) ? first : ch();
            selectTerminalChannel(nextSid, nextCh);
          }}
          aria-label="sid"
        >
          <option value="">{t("terminal.no_source")}</option>
          <For each={sidOptions()}>
            {(s) => <option value={s.sid}>{s.name}</option>}
          </For>
        </select>
        <select
          value={ch()}
          onChange={(e) => {
            selectTerminalChannel(sid(), Number(e.currentTarget.value));
          }}
          aria-label="ch"
          disabled={!hasActiveSource()}
        >
          <For each={chOptions()}>
            {(c) => <option value={c}>ch {c}</option>}
          </For>
        </select>
        <span style={{ color: "var(--wl-fg-muted)", "align-self": "center" }}>
          {t("terminal.target")}: {hasActiveSource() ? `${sid()} / ch ${ch()}` : t("terminal.no_source")}
        </span>
        <button type="button" onClick={clearTerminal} style={{ "margin-left": "auto" }}>
          {t("terminal.clear")}
        </button>
        <button type="button" onClick={copySelection}>
          {t("terminal.copy_selection")}
        </button>
      </div>
      <div ref={host!} style={{ flex: "1 1 auto", "min-height": 0 }} />
    </div>
  );
}
