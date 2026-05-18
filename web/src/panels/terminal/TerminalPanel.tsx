// Terminal panel: xterm.js + WebGL renderer (NFR-PERF-001).
// Subscribes to a (sid, ch), prints incoming bytes, and forwards user
// keystrokes back to the server via a `write` frame.
//
// REQ: FR-UI-002
// REQ: FR-UI-010
// REQ: FR-UI-011
// REQ: FR-UI-013

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
  pushToast,
  selectTerminalChannel,
  sendWrite,
  sourcesStore,
  terminalChannel,
  useChannel,
} from "~/state";
import { displaySettings, formatTimestampNs } from "~/state/displaySettings";
import { observeVisibility } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";

export interface TerminalPanelProps {
  sid: string;
  ch: number;
  followSelection?: boolean;
}

const encoder = new TextEncoder();

function sourceDisplayName(p: Pick<DataPayload, "sid" | "source">): string {
  return p.source ?? sourcesStore[p.sid]?.name ?? p.sid.slice(0, 8);
}

function metadataPrefix(p: DataPayload): string {
  const parts: string[] = [];
  if (displaySettings.showTimestamp) {
    parts.push(formatTimestampNs(p.ts_origin));
  }
  if (displaySettings.showKind) {
    const tags = p.tags && p.tags.length > 0 ? `:${p.tags.join("|")}` : "";
    parts.push(`${p.kind}${tags}`);
  }
  if (displaySettings.showSource) {
    parts.push(sourceDisplayName(p));
  }
  return parts.length > 0 ? `[${parts.join(" ")}] ` : "";
}

export function TerminalPanel(props: TerminalPanelProps) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;

  const [sid, setSid] = createSignal(props.sid);
  const [ch, setCh] = createSignal(props.ch);
  const [txText, setTxText] = createSignal("");

  const sidOptions = createMemo(() => Object.values(sourcesStore));
  const chOptions = createMemo(() => {
    const s = sourcesStore[sid()];
    return s ? s.channels : [ch()];
  });
  const hasActiveSource = createMemo(() => Boolean(sourcesStore[sid()]));
  const targetLabel = createMemo(() => {
    if (!hasActiveSource()) return t("terminal.no_source");
    const source = sourcesStore[sid()];
    const name = source?.name ?? sid().slice(0, 8);
    return `${name} / ch ${ch()} (${sid().slice(0, 8)})`;
  });

  function rebind(): void {
    unsub?.();
    unsub = null;
    if (!hasActiveSource()) return;
    unsub = useChannel(sid(), ch(), (p: DataPayload) => {
      const prefix = metadataPrefix(p);
      if (p.body instanceof Uint8Array) {
        if (prefix) term?.write(prefix);
        term?.write(p.body);
      } else if (typeof p.body === "object" && p.body) {
        term?.writeln(`${prefix}${JSON.stringify(p.body)}`);
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
    if (props.followSelection === false) return;
    const selected = terminalChannel();
    if (!selected) return;
    bind(selected.sid, selected.ch);
  });

  createEffect(() => {
    if (hasActiveSource()) return;
    const first = sidOptions()[0];
    if (!first) return;
    const firstCh = first.channels[0] ?? 0;
    if (props.followSelection === false) {
      bind(first.sid, firstCh);
    } else {
      selectTerminalChannel(first.sid, firstCh);
    }
  });

  function clearTerminal(): void {
    term?.clear();
  }

  function copySelection(): void {
    const text = term?.getSelection() ?? "";
    if (!text) return;
    void navigator.clipboard?.writeText(text);
  }

  function sendTextInput(): void {
    const text = txText();
    if (!hasActiveSource() || text.length === 0) return;
    const bytes = encoder.encode(text);
    try {
      sendWrite(sid(), ch(), bytes);
      setTxText("");
      pushToast({
        level: "info",
        message: `${t("terminal.sent")} (${bytes.byteLength} bytes)`,
      });
    } catch {
      pushToast({ level: "error", message: t("terminal.send_error") });
    }
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
      scrollback: displaySettings.terminalScrollback,
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

    const selected = props.followSelection === false ? null : terminalChannel();
    if (selected) {
      setSid(selected.sid);
      setCh(selected.ch);
    }
    rebind();
    reobserve();
  });

  createEffect(() => {
    const scrollback = displaySettings.terminalScrollback;
    if (term) term.options.scrollback = scrollback;
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
          {t("terminal.target")}: {targetLabel()}
        </span>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            sendTextInput();
          }}
          style={{ display: "flex", gap: "4px", "margin-left": "auto" }}
          aria-label={t("terminal.send_label")}
        >
          <input
            type="text"
            value={txText()}
            onInput={(e) => setTxText(e.currentTarget.value)}
            placeholder={t("terminal.send_placeholder")}
            disabled={!hasActiveSource()}
            style={{ width: "260px" }}
            aria-label={t("terminal.send_label")}
          />
          <button type="submit" disabled={!hasActiveSource() || txText().length === 0}>
            {t("terminal.send")}
          </button>
        </form>
        <button type="button" onClick={clearTerminal}>
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
