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
import { getChannelFrames } from "~/state/channelBuffers";
import { enabledClassificationRules } from "~/state/classificationRules";
import { displaySettings } from "~/state/displaySettings";
import {
  clientClassificationTags,
  DEFAULT_DISPLAY_FILTER,
  labelForSid,
  payloadMatchesFilter,
  renderPayload,
  sourceDisplayName,
  type DisplayFilter,
} from "~/state/displayFrames";
import { sourceAliases } from "~/state/sourceAliases";
import { sourceEncodings } from "~/state/sourceEncodings";
import { sourceStartOptions } from "~/state/sourceStartOptions";
import { observeVisibility } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";

export interface TerminalPanelProps {
  sid: string;
  ch: number;
  followSelection?: boolean;
}

const encoder = new TextEncoder();
const DATA_KINDS: DataPayload["kind"][] = ["bytes", "datagram", "frame", "record"];

export function TerminalPanel(props: TerminalPanelProps) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;
  let renderedRecords = 0;

  const [sid, setSid] = createSignal(props.sid);
  const [ch, setCh] = createSignal(props.ch);
  const [txText, setTxText] = createSignal("");
  const [filterKind, setFilterKind] = createSignal<DisplayFilter["kind"]>(
    DEFAULT_DISPLAY_FILTER.kind,
  );
  const [filterTag, setFilterTag] = createSignal(DEFAULT_DISPLAY_FILTER.tagQuery);
  const [filterSource, setFilterSource] = createSignal(DEFAULT_DISPLAY_FILTER.sourceQuery);

  const sidOptions = createMemo(() => Object.values(sourcesStore));
  const chOptions = createMemo(() => {
    const s = sourcesStore[sid()];
    return s ? s.channels : [ch()];
  });
  const hasActiveSource = createMemo(() => Boolean(sourcesStore[sid()]));
  const targetLabel = createMemo(() => {
    if (!hasActiveSource()) return t("terminal.no_source");
    return `${labelForSid(sid(), sourcesStore, sourceAliases)} / ch ${ch()}`;
  });
  const activeFilter = createMemo<DisplayFilter>(() => ({
    kind: filterKind(),
    tagQuery: filterTag(),
    sourceQuery: filterSource(),
  }));

  function isAtBottom(): boolean {
    const active = term?.buffer.active;
    if (!active) return true;
    return active.viewportY >= active.baseY - 1;
  }

  function writeRendered(text: string, newline: boolean): void {
    const follow = isAtBottom();
    const done = () => {
      if (follow) term?.scrollToBottom();
    };
    if (newline) {
      term?.writeln(text, done);
    } else {
      term?.write(text, done);
    }
  }

  function renderFrame(p: DataPayload, enforceLimit = true): void {
    const sourceLabel = sourceDisplayName(p, sourcesStore, sourceAliases);
    const encoding = sourceEncodings[p.sid]?.encoding ?? sourceStartOptions.encoding;
    const extraTags = clientClassificationTags(p, enabledClassificationRules(), encoding);
    if (!payloadMatchesFilter(p, activeFilter(), sourceLabel, extraTags)) return;
    const rendered = renderPayload(p, displaySettings, sourceLabel, { encoding, extraTags });
    renderedRecords += 1;
    if (enforceLimit && renderedRecords > displaySettings.terminalMaxRecords) {
      redrawFromBuffer();
      return;
    }
    writeRendered(rendered.text, rendered.newline);
  }

  function redrawFromBuffer(): void {
    if (!term || !hasActiveSource()) return;
    renderedRecords = 0;
    term.clear();
    for (const frame of getChannelFrames(sid(), ch(), displaySettings.terminalMaxRecords)) {
      renderFrame(frame, false);
    }
    term.scrollToBottom();
  }

  function rebind(): void {
    unsub?.();
    unsub = null;
    if (!hasActiveSource()) return;
    unsub = useChannel(sid(), ch(), (p: DataPayload) => {
      renderFrame(p);
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
    redrawFromBuffer();
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
    renderedRecords = 0;
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

  createEffect(() => {
    filterKind();
    filterTag();
    filterSource();
    displaySettings.showTimestamp;
    displaySettings.showKind;
    displaySettings.showSource;
    displaySettings.timezone;
    displaySettings.terminalMaxRecords;
    sourceEncodings[sid()]?.encoding;
    sourceStartOptions.encoding;
    enabledClassificationRules();
    redrawFromBuffer();
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
            if (props.followSelection === false) {
              bind(nextSid, nextCh);
            } else {
              selectTerminalChannel(nextSid, nextCh);
            }
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
            const nextCh = Number(e.currentTarget.value);
            if (props.followSelection === false) {
              bind(sid(), nextCh);
            } else {
              selectTerminalChannel(sid(), nextCh);
            }
          }}
          aria-label="ch"
          disabled={!hasActiveSource()}
        >
          <For each={chOptions()}>
            {(c) => <option value={c}>ch {c}</option>}
          </For>
        </select>
        <label>
          {t("terminal.filter_kind")} {" "}
          <select
            value={filterKind()}
            onChange={(e) => setFilterKind(e.currentTarget.value as DisplayFilter["kind"])}
            aria-label={t("terminal.filter_kind")}
          >
            <option value="all">{t("terminal.filter_all")}</option>
            <For each={DATA_KINDS}>
              {(kind) => <option value={kind}>{kind}</option>}
            </For>
          </select>
        </label>
        <input
          type="search"
          value={filterTag()}
          onInput={(e) => setFilterTag(e.currentTarget.value)}
          placeholder={t("terminal.filter_tag_placeholder")}
          aria-label={t("terminal.filter_tag")}
          style={{ width: "120px" }}
        />
        <input
          type="search"
          value={filterSource()}
          onInput={(e) => setFilterSource(e.currentTarget.value)}
          placeholder={t("terminal.filter_source_placeholder")}
          aria-label={t("terminal.filter_source")}
          style={{ width: "120px" }}
        />
        <span title={sid()} style={{ color: "var(--wl-fg-muted)", "align-self": "center" }}>
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
