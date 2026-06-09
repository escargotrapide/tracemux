// Terminal panel: xterm.js + WebGL renderer (NFR-PERF-001).
// Subscribes to a (sid, ch), prints incoming bytes, and forwards user
// keystrokes back to the server via a `write` frame.
//
// REQ: FR-UI-002
// REQ: FR-UI-010
// REQ: FR-UI-011
// REQ: FR-UI-013
// REQ: FR-UI-014
// REQ: FR-UI-018

import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { t } from "~/i18n";
import {
  pushToast,
  clearClientDisplayBuffers,
  connState,
  displayClearVersion,
  selectTerminalChannel,
  sendResize,
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
  mergedTags,
  payloadMatchesFilter,
  renderPayload,
  sourceDisplayName,
  type DisplayFilter,
} from "~/state/displayFrames";
import { sourceAliases } from "~/state/sourceAliases";
import {
  encodingForChannel,
  sourceEncodings,
  sourceEncodingsVersion,
  sourceEncodingKey,
  updateChannelEncoding,
} from "~/state/sourceEncodings";
import { sourceStartOptions, SUPPORTED_SOURCE_ENCODINGS } from "~/state/sourceStartOptions";
import {
  LOCAL_ECHO_MODES,
  NEWLINE_MODES,
  presetLocalEcho,
  presetNewline,
  resolvedLocalEcho,
  resolvedNewlineBytes,
  seedTerminalInputDefaults,
  terminalInputFor,
  terminalInputVersion,
  updateTerminalInput,
  type LocalEchoMode,
  type NewlineMode,
} from "~/state/terminalInput";
import { observeVisibility } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";

export interface TerminalPanelProps {
  sid: string;
  ch: number;
  followSelection?: boolean;
}

const encoder = new TextEncoder();
const DATA_KINDS: DataPayload["kind"][] = ["bytes", "datagram", "frame", "record"];
const LOG_TYPE_ALL = "all";
const LOG_TYPE_CUSTOM = "custom";
// Re-decoding and redrawing the whole buffer is cheap for small buffers, so the
// confirmation only appears once a channel holds enough records to make the
// full repaint noticeable.
const ENCODING_CHANGE_CONFIRM_THRESHOLD = 500;
// Text-filter inputs (tag / source) drive a full buffer redraw, so typing is
// debounced to keep the terminal responsive while the user is still typing.
const FILTER_REDRAW_DEBOUNCE_MS = 200;

type LogTypeSelection = typeof LOG_TYPE_ALL | typeof LOG_TYPE_CUSTOM | `kind:${DataPayload["kind"]}` | `tag:${string}`;

function connectionDetail(status: string): string {
  if (status === "connecting") return t("status.connecting_detail");
  if (status === "closed") return t("status.closed_detail");
  if (status === "error") return t("status.error_detail");
  return t(`status.${status}`);
}

function connectionTone(status: string): "ok" | "warn" | "err" {
  if (status === "open") return "ok";
  if (status === "connecting" || status === "idle") return "warn";
  return "err";
}

function kindSelection(kind: DataPayload["kind"]): LogTypeSelection {
  return `kind:${kind}`;
}

function tagSelection(tag: string): LogTypeSelection {
  return `tag:${tag}`;
}

function logTypeLabel(selection: LogTypeSelection): string {
  if (selection === LOG_TYPE_ALL) return t("terminal.filter_all");
  if (selection === LOG_TYPE_CUSTOM) return t("terminal.log_type_custom");
  if (selection.startsWith("kind:")) {
    return `${t("terminal.log_type_kind")}: ${selection.slice("kind:".length)}`;
  }
  return `${t("terminal.log_type_tag")}: ${selection.slice("tag:".length)}`;
}

export function TerminalPanel(props: TerminalPanelProps) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;
  let renderedRecords = 0;
  let lastSendErrorToastMs = 0;
  // Local cooked-mode line buffer for pipe-based process sources (cmd.exe,
  // PowerShell). These do not echo stdin and have no line discipline, so the
  // panel echoes typed input locally and only sends a completed line on Enter.
  let inputLine = "";

  const [sid, setSid] = createSignal(props.sid);
  const [ch, setCh] = createSignal(props.ch);
  const [txText, setTxText] = createSignal("");
  const [filterKind, setFilterKind] = createSignal<DisplayFilter["kind"]>(
    DEFAULT_DISPLAY_FILTER.kind,
  );
  const [filterTag, setFilterTag] = createSignal(DEFAULT_DISPLAY_FILTER.tagQuery);
  const [filterSource, setFilterSource] = createSignal(DEFAULT_DISPLAY_FILTER.sourceQuery);
  // Debounced mirrors of the text filters above. The redraw effect and the
  // live-render filter read these so a full buffer repaint only happens after
  // typing settles, not on every keystroke.
  const [debouncedFilterTag, setDebouncedFilterTag] = createSignal(
    DEFAULT_DISPLAY_FILTER.tagQuery,
  );
  const [debouncedFilterSource, setDebouncedFilterSource] = createSignal(
    DEFAULT_DISPLAY_FILTER.sourceQuery,
  );
  const [logTypeSelection, setLogTypeSelection] = createSignal<LogTypeSelection>(LOG_TYPE_ALL);
  const [bufferVersion, setBufferVersion] = createSignal(0);
  const [encodingResetVersion, setEncodingResetVersion] = createSignal(0);

  const sidOptions = createMemo(() => Object.values(sourcesStore));
  const chOptions = createMemo(() => {
    const s = sourcesStore[sid()];
    return s ? s.channels : [ch()];
  });
  const hasActiveSource = createMemo(() => Boolean(sourcesStore[sid()]));
  const connectionStatus = createMemo(() => connState().status);
  const canSend = createMemo(() => hasActiveSource() && connectionStatus() === "open");
  const sendUnavailableReason = createMemo(() => {
    if (!hasActiveSource()) return t("terminal.no_source");
    if (connectionStatus() !== "open") return connectionDetail(connectionStatus());
    return "";
  });
  const currentEncodingFallback = createMemo(() => {
    sourceEncodingsVersion();
    return sourceEncodings[sourceEncodingKey(sid())]?.encoding
      ?? sourcesStore[sid()]?.encoding
      ?? sourceStartOptions.encoding;
  });
  const currentEncoding = createMemo(() => {
    sourceEncodingsVersion();
    return encodingForChannel(sid(), ch(), currentEncodingFallback());
  });
  const encodingSelectValue = createMemo(() => {
    encodingResetVersion();
    return currentEncoding();
  });
  const encodingOptions = createMemo(() => {
    const options = [...SUPPORTED_SOURCE_ENCODINGS] as string[];
    const current = currentEncoding();
    return options.includes(current) ? options : [current, ...options];
  });
  const localEchoValue = createMemo<LocalEchoMode>(() => {
    terminalInputVersion();
    return terminalInputFor(sid()).localEcho;
  });
  const newlineValue = createMemo<NewlineMode>(() => {
    terminalInputVersion();
    return terminalInputFor(sid()).newline;
  });
  // A PTY source is a real terminal: it echoes stdin and has its own line
  // discipline, so the panel forwards bytes raw and forwards resize events
  // instead of running a local cooked mode.
  const isPty = createMemo(() => sourcesStore[sid()]?.kind === "pty");
  const localEchoAutoLabel = createMemo(() => {
    const kind = sourcesStore[sid()]?.kind ?? "";
    return presetLocalEcho(kind) === "on"
      ? t("terminal.input_on")
      : t("terminal.input_off");
  });
  const newlineAutoLabel = createMemo(() => {
    const kind = sourcesStore[sid()]?.kind ?? "";
    const name = sourcesStore[sid()]?.name ?? "";
    return t(`terminal.newline_${presetNewline(kind, name)}`);
  });
  const targetLabel = createMemo(() => {
    if (!hasActiveSource()) return t("terminal.no_source");
    return `${labelForSid(sid(), sourcesStore, sourceAliases)} / ch ${ch()}`;
  });
  const activeFilter = createMemo<DisplayFilter>(() => ({
    kind: filterKind(),
    tagQuery: debouncedFilterTag(),
    sourceQuery: debouncedFilterSource(),
  }));
  const logTypeOptions = createMemo(() => {
    bufferVersion();
    const options = new Map<LogTypeSelection, string>();
    options.set(LOG_TYPE_ALL, logTypeLabel(LOG_TYPE_ALL));
    for (const kind of DATA_KINDS) {
      const selection = kindSelection(kind);
      options.set(selection, logTypeLabel(selection));
    }
    for (const frame of getChannelFrames(sid(), ch(), displaySettings.terminalMaxRecords)) {
      const fallback = sourcesStore[frame.sid]?.encoding ?? sourceStartOptions.encoding;
      const encoding = encodingForChannel(frame.sid, frame.ch, fallback);
      const tags = mergedTags(
        frame,
        clientClassificationTags(frame, enabledClassificationRules(), encoding),
      );
      for (const tag of tags) {
        const selection = tagSelection(tag);
        options.set(selection, logTypeLabel(selection));
      }
    }
    if (logTypeSelection() === LOG_TYPE_CUSTOM) {
      options.set(LOG_TYPE_CUSTOM, logTypeLabel(LOG_TYPE_CUSTOM));
    }
    return [...options.entries()].map(([value, label]) => ({ value, label }));
  });

  function applyLogTypeSelection(selection: LogTypeSelection): void {
    setLogTypeSelection(selection);
    // Dropdown-driven changes should redraw immediately (no typing in flight),
    // so flush the debounced mirrors alongside the raw filter signals.
    const setTag = (value: string): void => {
      setFilterTag(value);
      setDebouncedFilterTag(value);
    };
    if (selection === LOG_TYPE_ALL) {
      setFilterKind("all");
      setTag("");
      return;
    }
    if (selection === LOG_TYPE_CUSTOM) return;
    if (selection.startsWith("kind:")) {
      setFilterKind(selection.slice("kind:".length) as DataPayload["kind"]);
      setTag("");
      return;
    }
    setFilterKind("all");
    setTag(selection.slice("tag:".length));
  }

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
    const fallback = sourcesStore[p.sid]?.encoding ?? sourceStartOptions.encoding;
    const encoding = encodingForChannel(p.sid, p.ch, fallback);
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
    term.reset();
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
      setBufferVersion((value) => value + 1);
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

  function clearClientDisplay(): void {
    clearClientDisplayBuffers();
    renderedRecords = 0;
    term?.clear();
    pushToast({ level: "info", message: t("display.clear_requested") });
  }

  function copySelection(): void {
    const text = term?.getSelection() ?? "";
    if (!text) return;
    void navigator.clipboard?.writeText(text);
  }

  function sendTextInput(): void {
    const text = txText();
    if (!hasActiveSource() || text.length === 0) return;
    if (!canSend()) {
      showSendErrorToast();
      return;
    }
    const bytes = encoder.encode(text);
    if (sendWrite(sid(), ch(), bytes)) {
      setTxText("");
      pushToast({
        level: "info",
        message: `${t("terminal.sent")} (${bytes.byteLength} bytes)`,
      });
    } else {
      showSendErrorToast();
    }
  }

  function updateTerminalEncoding(encoding: string): void {
    if (!hasActiveSource()) return;
    if (encoding === currentEncoding()) return;
    const bufferedRecords = getChannelFrames(
      sid(),
      ch(),
      displaySettings.terminalMaxRecords,
    ).length;
    if (
      bufferedRecords >= ENCODING_CHANGE_CONFIRM_THRESHOLD &&
      !window.confirm(
        t("terminal.encoding_change_confirm").replace("{count}", String(bufferedRecords)),
      )
    ) {
      // Restore the controlled <select> to the unchanged value.
      setEncodingResetVersion((v) => v + 1);
      return;
    }
    updateChannelEncoding(
      sid(),
      ch(),
      encoding,
      undefined,
      Date.now(),
      currentEncodingFallback(),
    );
  }

  function showSendErrorToast(): void {
    const now = Date.now();
    if (now - lastSendErrorToastMs < 1_500) return;
    lastSendErrorToastMs = now;
    pushToast({ level: "error", message: t("terminal.send_error") });
  }

  function safeFit(): void {
    try {
      fit?.fit();
    } catch {
      // Dockview may still be settling panel dimensions; the next
      // ResizeObserver tick will retry. Keep the UI smoke-test quiet.
    }
    // Forward the new terminal size to PTY sources so the child repaints.
    if (isPty() && canSend() && term) {
      sendResize(sid(), ch(), term.cols, term.rows);
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
      if (!canSend()) {
        if (hasActiveSource()) showSendErrorToast();
        return;
      }
      // A PTY source is a real terminal (echoes stdin, has line discipline):
      // forward keystrokes raw, exactly as xterm produced them.
      if (sourcesStore[sid()]?.kind === "pty") {
        if (!sendWrite(sid(), ch(), encoder.encode(data))) showSendErrorToast();
        return;
      }
      // Pipe-based process sources (cmd.exe, PowerShell) do not echo stdin and
      // have no line discipline. The local echo and line-ending behaviour is
      // configurable per source (see the Local echo / Line ending selectors);
      // `auto` resolves to a preset based on the source kind/name.
      const k = sourcesStore[sid()]?.kind ?? "";
      const name = sourcesStore[sid()]?.name ?? "";
      const newlineBytes = resolvedNewlineBytes(sid(), k, name);
      if (!resolvedLocalEcho(sid(), k)) {
        // Raw mode: forward keystrokes as-is, translating Enter to the chosen
        // line ending so the line-ending selector still applies.
        const payload = data === "\r" ? newlineBytes : data;
        if (!sendWrite(sid(), ch(), encoder.encode(payload))) showSendErrorToast();
        return;
      }
      // Cooked mode: echo typed input locally, support Backspace, and send a
      // whole line terminated by the chosen line ending on Enter.
      for (const c of data) {
        if (c === "\r" || c === "\n") {
          term!.write("\r\n");
          const line = `${inputLine}${newlineBytes}`;
          inputLine = "";
          if (!sendWrite(sid(), ch(), encoder.encode(line))) showSendErrorToast();
        } else if (c === "\x7f" || c === "\b") {
          if (inputLine.length > 0) {
            inputLine = inputLine.slice(0, -1);
            term!.write("\b \b");
          }
        } else if (c === "\x03") {
          // Ctrl+C: abandon the local line and forward the interrupt byte.
          term!.write("^C\r\n");
          inputLine = "";
          if (!sendWrite(sid(), ch(), encoder.encode("\x03"))) showSendErrorToast();
        } else if (c >= " ") {
          inputLine += c;
          term!.write(c);
        }
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

  // Drop any half-typed local input when the target source/channel changes so
  // the cooked-mode buffer never leaks across sources.
  createEffect(() => {
    sid();
    ch();
    inputLine = "";
  });

  // Seed local echo / line ending from server-declared config defaults the
  // first time a source is selected (user overrides always win).
  createEffect(() => {
    const s = sourcesStore[sid()];
    if (s) seedTerminalInputDefaults(s.sid, s.localEchoDefault, s.newlineDefault);
  });

  createEffect(() => {
    const tag = filterTag();
    const source = filterSource();
    const handle = setTimeout(() => {
      setDebouncedFilterTag(tag);
      setDebouncedFilterSource(source);
    }, FILTER_REDRAW_DEBOUNCE_MS);
    onCleanup(() => clearTimeout(handle));
  });

  createEffect(() => {
    filterKind();
    debouncedFilterTag();
    debouncedFilterSource();
    displaySettings.showTimestamp;
    displaySettings.showKind;
    displaySettings.showSource;
    displaySettings.timezone;
    displaySettings.terminalMaxRecords;
    displayClearVersion();
    currentEncoding();
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
      <div class="wl-terminal-toolbar">
        <select
          class="wl-terminal-select wl-terminal-source-select"
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
          class="wl-terminal-select wl-terminal-ch-select"
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
        <label class="wl-terminal-field">
          <span class="wl-terminal-field-label">{t("terminal.encoding")}</span>
          <select
            class="wl-terminal-select wl-terminal-encoding-select"
            value={encodingSelectValue()}
            onChange={(e) => updateTerminalEncoding(e.currentTarget.value)}
            aria-label={t("terminal.encoding")}
            disabled={!hasActiveSource()}
          >
            <For each={encodingOptions()}>
              {(encoding) => <option value={encoding}>{encoding}</option>}
            </For>
          </select>
        </label>
        <Show when={!isPty()}>
        <label class="wl-terminal-field">
          <span class="wl-terminal-field-label">{t("terminal.local_echo")}</span>
          <select
            class="wl-terminal-select wl-terminal-local-echo-select"
            value={localEchoValue()}
            onChange={(e) =>
              updateTerminalInput(sid(), {
                localEcho: e.currentTarget.value as LocalEchoMode,
              })
            }
            aria-label={t("terminal.local_echo")}
            disabled={!hasActiveSource()}
          >
            <For each={LOCAL_ECHO_MODES}>
              {(mode) => (
                <option value={mode}>
                  {mode === "auto"
                    ? `${t("terminal.input_auto")} (${localEchoAutoLabel()})`
                    : t(`terminal.input_${mode}`)}
                </option>
              )}
            </For>
          </select>
        </label>
        <label class="wl-terminal-field">
          <span class="wl-terminal-field-label">{t("terminal.newline")}</span>
          <select
            class="wl-terminal-select wl-terminal-newline-select"
            value={newlineValue()}
            onChange={(e) =>
              updateTerminalInput(sid(), {
                newline: e.currentTarget.value as NewlineMode,
              })
            }
            aria-label={t("terminal.newline")}
            disabled={!hasActiveSource()}
          >
            <For each={NEWLINE_MODES}>
              {(mode) => (
                <option value={mode}>
                  {mode === "auto"
                    ? `${t("terminal.input_auto")} (${newlineAutoLabel()})`
                    : t(`terminal.newline_${mode}`)}
                </option>
              )}
            </For>
          </select>
        </label>
        </Show>
        <label class="wl-terminal-field">
          <span class="wl-terminal-field-label">{t("terminal.log_type_switch")}</span>
          <select
            class="wl-terminal-select wl-terminal-log-type-select"
            value={logTypeSelection()}
            onChange={(e) => applyLogTypeSelection(e.currentTarget.value as LogTypeSelection)}
            aria-label={t("terminal.log_type_switch")}
          >
            <For each={logTypeOptions()}>
              {(option) => <option value={option.value}>{option.label}</option>}
            </For>
          </select>
        </label>
        <label class="wl-terminal-field">
          <span class="wl-terminal-field-label">{t("terminal.filter_kind")}</span>
          <select
            class="wl-terminal-select wl-terminal-kind-select"
            value={filterKind()}
            onChange={(e) => {
              setFilterKind(e.currentTarget.value as DisplayFilter["kind"]);
              setLogTypeSelection(LOG_TYPE_CUSTOM);
            }}
            aria-label={t("terminal.filter_kind")}
          >
            <option value="all">{t("terminal.filter_all")}</option>
            <For each={DATA_KINDS}>
              {(kind) => <option value={kind}>{kind}</option>}
            </For>
          </select>
        </label>
        <input
          class="wl-terminal-search"
          type="search"
          value={filterTag()}
          onInput={(e) => {
            setFilterTag(e.currentTarget.value);
            setLogTypeSelection(e.currentTarget.value.trim() ? LOG_TYPE_CUSTOM : LOG_TYPE_ALL);
          }}
          placeholder={t("terminal.filter_tag_placeholder")}
          aria-label={t("terminal.filter_tag")}
        />
        <input
          class="wl-terminal-search"
          type="search"
          value={filterSource()}
          onInput={(e) => setFilterSource(e.currentTarget.value)}
          placeholder={t("terminal.filter_source_placeholder")}
          aria-label={t("terminal.filter_source")}
        />
        <span class="wl-terminal-target" title={sid()}>
          {t("terminal.target")}: {targetLabel()}
        </span>
        <span
          class={`wl-terminal-connection wl-terminal-connection-${connectionStatus()}`}
          title={connectionDetail(connectionStatus())}
          aria-label={`${t("metrics.connection")}: ${t(`status.${connectionStatus()}`)}`}
        >
          <span class={`wl-status-dot ${connectionTone(connectionStatus())}`} />
          {t(`status.${connectionStatus()}`)}
        </span>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            sendTextInput();
          }}
          class="wl-terminal-send"
          aria-label={t("terminal.send_label")}
        >
          <input
            class="wl-terminal-send-input"
            type="text"
            value={txText()}
            onInput={(e) => setTxText(e.currentTarget.value)}
            placeholder={t("terminal.send_placeholder")}
            disabled={!canSend()}
            title={sendUnavailableReason() || undefined}
            aria-label={t("terminal.send_label")}
          />
          <button
            type="submit"
            disabled={!canSend() || txText().length === 0}
            title={sendUnavailableReason() || undefined}
          >
            {t("terminal.send")}
          </button>
        </form>
        <div class="wl-terminal-actions">
          <button type="button" onClick={clearClientDisplay}>
            {t("terminal.clear")}
          </button>
          <button type="button" onClick={copySelection}>
            {t("terminal.copy_selection")}
          </button>
        </div>
      </div>
      <div ref={host!} style={{ flex: "1 1 auto", "min-height": 0 }} />
    </div>
  );
}
