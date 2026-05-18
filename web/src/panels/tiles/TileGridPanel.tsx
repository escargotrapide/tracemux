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
import {
  displaySettings,
  formatTimestampNs,
  updateDisplaySettings,
} from "~/state/displaySettings";
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

  const label = createMemo(() => sourcesStore[props.sid]?.name ?? props.sid.slice(0, 8));

  function safeFit(): void {
    try {
      fit?.fit();
    } catch {
      // The tile can be resized while Dockview is settling. The next
      // ResizeObserver tick will retry.
    }
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
      parts.push(p.source ?? label());
    }
    return parts.length > 0 ? `[${parts.join(" ")}] ` : "";
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
      const prefix = metadataPrefix(p);
      if (p.body instanceof Uint8Array) {
        if (prefix) term?.write(prefix);
        term?.write(p.body);
      } else if (typeof p.body === "object" && p.body) {
        term?.writeln(`${prefix}${JSON.stringify(p.body)}`);
      }
    });
    unobserve = observeVisibility(host, { sid: props.sid, ch: props.ch });
    resizeObs = new ResizeObserver(() => requestAnimationFrame(safeFit));
    resizeObs.observe(host);
  });

  createEffect(() => {
    const scrollback = displaySettings.tileScrollback;
    if (term) term.options.scrollback = scrollback;
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
