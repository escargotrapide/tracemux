// 16-tile grid for high-cardinality monitoring. Each tile is a tiny
// xterm bound to a (sid, ch). Off-screen tiles report
// `panel_priority{visible:false}` so the server can switch them to
// the slow coalescing bucket (NFR-PERF-001).
//
// REQ: FR-UI-012

import { createMemo, For, onCleanup, onMount } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { sourcesStore, useChannel } from "~/state";
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
  let unsub: (() => void) | null = null;
  let unobserve: (() => void) | null = null;

  onMount(() => {
    term = new Terminal({
      convertEol: true,
      cursorBlink: false,
      fontSize: 10,
      scrollback: 500,
      theme: { background: "#0e1116", foreground: "#c9d1d9" },
    });
    term.open(host);
    unsub = useChannel(props.sid, props.ch, (p: DataPayload) => {
      if (p.body instanceof Uint8Array) term?.write(p.body);
    });
    unobserve = observeVisibility(host, { sid: props.sid, ch: props.ch });
  });

  onCleanup(() => {
    unobserve?.();
    unsub?.();
    term?.dispose();
  });

  return (
    <div class="wl-tile" data-sid={props.sid} data-ch={props.ch}>
      <div class="wl-tile-header">
        {props.sid.slice(0, 8)} / ch {props.ch}
      </div>
      <div ref={host!} class="wl-tile-body" />
    </div>
  );
}

export function TileGridPanel() {
  const tiles = createMemo(deriveBindings);

  return (
    <div class="wl-tile-grid" data-testid="tile-grid">
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
  );
}
