// Global UI state. The server is the single source of truth ? these
// signals only mirror what arrives over the wire.

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import {
  WireClient,
  resolveWanloggerUrl,
  type ConnState,
  type DataPayload,
  type Frame,
} from "~/adapters/wss";

export interface SourceInfo {
  sid: string;
  name: string;
  kind: string;
  channels: number[];
  lastTsMs: number;
  bytesIn: number;
}

export interface ChannelKey {
  sid: string;
  ch: number;
}

const [conn, setConn] = createSignal<ConnState>({ status: "idle" });
const [sources, setSources] = createStore<Record<string, SourceInfo>>({});

let client: WireClient | null = null;

const channelListeners = new Map<string, Set<(p: DataPayload) => void>>();

function keyOf(sid: string, ch: number): string {
  return `${sid}/${ch}`;
}

export function getClient(): WireClient {
  if (!client) {
    const tokenEnv = import.meta.env.VITE_WANLOGGER_TOKEN;
    client = new WireClient({
      url: resolveWanloggerUrl(),
      ...(tokenEnv !== undefined ? { token: tokenEnv } : {}),
    });
    client.onState(setConn);
    client.onFrame(handleFrame);
    client.connect();
  }
  return client;
}

function handleFrame(frame: Frame): void {
  if (frame.type === "data") {
    const p = frame.payload as DataPayload;
    const key = keyOf(p.sid, p.ch);
    const ls = channelListeners.get(key);
    if (ls) for (const fn of ls) fn(p);

    // Light-weight per-source aggregate.
    const existing = sources[p.sid];
    const size =
      p.body instanceof Uint8Array
        ? p.body.byteLength
        : Object.keys(p.body ?? {}).length;
    if (existing) {
      setSources(p.sid, {
        ...existing,
        lastTsMs: Number(p.ts_ingest) / 1_000_000,
        bytesIn: existing.bytesIn + size,
        channels: existing.channels.includes(p.ch)
          ? existing.channels
          : [...existing.channels, p.ch],
      });
    } else {
      setSources(p.sid, {
        sid: p.sid,
        name: p.source ?? p.sid.slice(0, 8),
        kind: p.kind,
        channels: [p.ch],
        lastTsMs: Number(p.ts_ingest) / 1_000_000,
        bytesIn: size,
      });
    }
    return;
  }
  if (frame.type === "ctl") {
    // surfaced via state only for now
    return;
  }
}

/** Subscribe to a (sid, ch) channel. Returns an unsubscribe fn. */
export function useChannel(
  sid: string,
  ch: number,
  cb: (p: DataPayload) => void,
): () => void {
  const key = keyOf(sid, ch);
  let set = channelListeners.get(key);
  if (!set) {
    set = new Set();
    channelListeners.set(key, set);
    getClient().send({ type: "sub", sid, ch, payload: {} });
  }
  set.add(cb);
  return () => {
    const s = channelListeners.get(key);
    if (!s) return;
    s.delete(cb);
    if (s.size === 0) {
      channelListeners.delete(key);
      client?.send({ type: "unsub", sid, ch, payload: {} });
    }
  };
}

export const connState = conn;
export const sourcesStore = sources;
