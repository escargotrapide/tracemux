// 16-tile grid for high-cardinality monitoring. Each tile is a tiny
// xterm bound to a (sid, ch). Off-screen tiles report
// `panel_priority{visible:false}` so the server can switch them to
// the slow coalescing bucket (NFR-PERF-001).
//
// REQ: FR-UI-012

import { createEffect, createMemo, For, onCleanup, onMount } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { sourcesStore, useChannel } from "~/state";
import { getChannelFrames } from "~/state/channelBuffers";
import {
  displaySettings,
  updateDisplaySettings,
} from "~/state/displaySettings";
import {
  labelForSid,
  renderPayload,
  sourceDisplayName,
} from "~/state/displayFrames";
import { sourceAliases } from "~/state/sourceAliases";
import { observeVisibility, TILE_COUNT } from "~/state/visibility";
import type { DataPayload } from "~/adapters/wss";
import { t } from "~/i18n";

interface TileBinding {
  sid: string;
  ch: number;
}

function deriveBindings(): TileBinding[] {
  const out: TileBinding[] = [];
  for (const s of Object.values(sourcesStore)) {
    for (const ch of s.channels) {
      out.push({ sid: s.sid, ch });
      if (out.length >= TILE_COUNT) return out;
    }
  }
  return out;
}

function Tile(props: TileBinding) {
  let host!: HTMLDivElement;
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;
  let renderedRecords = 0;

  const label = createMemo(() => labelForSid(props.sid, sourcesStore, sourceAliases));

  function safeFit(): void {
    try {
      fit?.fit();
    } catch {
      // The tile can be resized while Dockview is settling. The next
      // ResizeObserver tick will retry.
    }
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
    const rendered = renderPayload(p, displaySettings, sourceLabel);
    renderedRecords += 1;
    if (enforceLimit && renderedRecords > displaySettings.tileMaxRecords) {
      redrawFromBuffer();
      return;
    }
    writeRendered(rendered.text, rendered.newline);
  }

  function redrawFromBuffer(): void {
    if (!term) return;
    renderedRecords = 0;
    term.clear();
    for (const frame of getChannelFrames(props.sid, props.ch, displaySettings.tileMaxRecords)) {
      renderFrame(frame, false);
    }
    term.scrollToBottom();
  }

  onMount(() => {
    term = new Terminal({
      convertEol: true,
      cursorBlink: false,
      fontSize: 10,
      scrollback: displaySettings.tileScrollback,
      theme: { background: "#0e1116", foreground: "#c9d1d9" },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    requestAnimationFrame(safeFit);
    unsub = useChannel(props.sid, props.ch, (p: DataPayload) => {
      renderFrame(p);
    });
    redrawFromBuffer();
    unobserve = observeVisibility(host, { sid: props.sid, ch: props.ch });
    resizeObs = new ResizeObserver(() => requestAnimationFrame(safeFit));
    resizeObs.observe(host);
  });

  createEffect(() => {
    const scrollback = displaySettings.tileScrollback;
    if (term) term.options.scrollback = scrollback;
    displaySettings.showTimestamp;
    displaySettings.showKind;
    displaySettings.showSource;
    displaySettings.timezone;
    displaySettings.tileMaxRecords;
    redrawFromBuffer();
    requestAnimationFrame(safeFit);
  });

  onCleanup(() => {
    unobserve?.();
    unsub?.();
    resizeObs?.disconnect();
    term?.dispose();
  });

  return (
    <div class="wl-tile" data-sid={props.sid} data-ch={props.ch}>
      <div class="wl-tile-header">
        {label()} / ch {props.ch}
      </div>
      <div ref={host!} class="wl-tile-body" />
    </div>
  );
}

export function TileGridPanel() {
  const tiles = createMemo(deriveBindings);
  const gridStyle = () => ({
    "grid-template-columns": `repeat(auto-fit, minmax(${displaySettings.tileMinWidth}px, 1fr))`,
    "grid-auto-rows": `minmax(${displaySettings.tileMinHeight}px, 1fr)`,
  });

  return (
    <div class="wl-tile-panel">
      <div class="wl-tile-toolbar">
        <label>
          {t("tiles.min_width")} {" "}
          <input
            type="number"
            min="120"
            max="1200"
            value={displaySettings.tileMinWidth}
            onInput={(ev) => updateDisplaySettings({ tileMinWidth: Number(ev.currentTarget.value) })}
          />
        </label>
        <label>
          {t("tiles.min_height")} {" "}
          <input
            type="number"
            min="80"
            max="900"
            value={displaySettings.tileMinHeight}
            onInput={(ev) => updateDisplaySettings({ tileMinHeight: Number(ev.currentTarget.value) })}
          />
        </label>
      </div>
      <div class="wl-tile-grid" data-testid="tile-grid" style={gridStyle()}>
        {tiles().length === 0 ? (
          <div style={{ color: "var(--wl-fg-muted)", padding: "8px" }}>
            {t("tiles.empty")}
          </div>
        ) : (
          <For each={tiles()}>
            {(b) => <Tile sid={b.sid} ch={b.ch} />}
          </For>
        )}
      </div>
    </div>
  );
}
