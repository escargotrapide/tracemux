// Global UI state. The server is the single source of truth -- these
// signals only mirror what arrives over the wire.

import { batch, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import {
  WireClient,
  resolveWanloggerUrl,
  type ConnState,
  type CtlPayload,
  type DataPayload,
  type Frame,
  type MetricsPayload,
  type SourceSyncPayload,
} from "~/adapters/wss";

export interface SourceInfo {
  sid: string;
  name: string;
  kind: string;
  status: "running" | "stopped" | "unknown";
  channels: number[];
  lastTsMs: number;
  bytesIn: number;
}

export interface ChannelKey {
  sid: string;
  ch: number;
}

export interface TerminalFocusRequest extends ChannelKey {
  id: number;
}

export interface UiPerfSnapshot {
  framesTotal: number;
  dataFrames: number;
  ctlFrames: number;
  metricsFrames: number;
  sourceUpdates: number;
  sourceSyncs: number;
  subscriptionDispatches: number;
  toastsPushed: number;
  toastsDropped: number;
  toastsDismissed: number;
  activeSubscriptions: number;
  toastCount: number;
  maxToasts: number;
  lastFrameTsMs: number;
}

const [conn, setConn] = createSignal<ConnState>({ status: "idle" });
const [sources, setSources] = createStore<Record<string, SourceInfo>>({});
const [metrics, setMetrics] = createSignal<MetricsPayload | null>(null);
const [terminalChannelState, setTerminalChannelState] = createSignal<ChannelKey | null>(null);
const [terminalFocusRequestState, setTerminalFocusRequestState] =
  createSignal<TerminalFocusRequest | null>(null);
let terminalFocusSeq = 1;

const uiPerfCounters: UiPerfSnapshot = {
  framesTotal: 0,
  dataFrames: 0,
  ctlFrames: 0,
  metricsFrames: 0,
  sourceUpdates: 0,
  sourceSyncs: 0,
  subscriptionDispatches: 0,
  toastsPushed: 0,
  toastsDropped: 0,
  toastsDismissed: 0,
  activeSubscriptions: 0,
  toastCount: 0,
  maxToasts: 64,
  lastFrameTsMs: 0,
};
const [uiPerf, setUiPerf] = createSignal<UiPerfSnapshot>({ ...uiPerfCounters });
let uiPerfPublishQueued = false;

export interface ToastInfo {
  id: number;
  level: "info" | "warn" | "error";
  message: string;
  errorId?: string;
  ts: number;
}
const [toasts, setToasts] = createStore<ToastInfo[]>([]);
const MAX_TOASTS = 64;
let toastSeq = 1;

uiPerfCounters.maxToasts = MAX_TOASTS;

function uiPerfSnapshot(): UiPerfSnapshot {
  return {
    ...uiPerfCounters,
    activeSubscriptions: channelListeners.size,
    toastCount: toasts.length,
    maxToasts: MAX_TOASTS,
  };
}

function publishUiPerf(): void {
  uiPerfPublishQueued = false;
  setUiPerf(uiPerfSnapshot());
}

function queueUiPerfPublish(): void {
  if (uiPerfPublishQueued) return;
  uiPerfPublishQueued = true;
  queueMicrotask(publishUiPerf);
}

function recordUiPerf(update: (snapshot: UiPerfSnapshot) => void): void {
  update(uiPerfCounters);
  queueUiPerfPublish();
}

export function pushToast(t: Omit<ToastInfo, "id" | "ts">): number {
  const id = toastSeq++;
  const dropped = Math.max(0, toasts.length + 1 - MAX_TOASTS);
  setToasts((prev) => [...prev, { ...t, id, ts: Date.now() }].slice(-MAX_TOASTS));
  recordUiPerf((p) => {
    p.toastsPushed += 1;
    p.toastsDropped += dropped;
  });
  return id;
}

export function dismissToast(id: number): void {
  setToasts((prev) => prev.filter((t) => t.id !== id));
  recordUiPerf((p) => {
    p.toastsDismissed += 1;
  });
}

export const toastsStore = toasts;

let client: WireClient | null = null;

const channelListeners = new Map<string, Set<(p: DataPayload) => void>>();

function keyOf(sid: string, ch: number): string {
  return `${sid}/${ch}`;
}

function upsertSourceStatus(
  sid: string,
  status: SourceInfo["status"],
  name?: string,
): void {
  const existing = sources[sid];
  if (existing) {
    setSources(sid, "status", status);
    return;
  }
  setSources(sid, {
    sid,
    name: name ?? sid.slice(0, 8),
    kind: "unknown",
    status,
    channels: [],
    lastTsMs: 0,
    bytesIn: 0,
  });
}

function removeSource(sid: string): void {
  setSources(sid, undefined as unknown as SourceInfo);
}

function syncSources(items: SourceSyncPayload[]): void {
  recordUiPerf((p) => {
    p.sourceSyncs += 1;
  });
  const seen = new Set<string>();
  for (const item of items) {
    if (!item.sid) continue;
    seen.add(item.sid);
    const existing = sources[item.sid];
    setSources(item.sid, {
      sid: item.sid,
      name: item.name ?? existing?.name ?? item.sid.slice(0, 8),
      kind: item.kind ?? existing?.kind ?? "unknown",
      status: item.status ?? existing?.status ?? "unknown",
      channels: item.channels && item.channels.length > 0 ? item.channels : existing?.channels ?? [0],
      lastTsMs: item.last_ts_ms ?? existing?.lastTsMs ?? 0,
      bytesIn: item.bytes_in ?? existing?.bytesIn ?? 0,
    });
  }
  for (const sid of Object.keys(sources)) {
    if (!seen.has(sid)) removeSource(sid);
  }
}

function resubscribeChannels(): void {
  for (const key of channelListeners.keys()) {
    const slash = key.lastIndexOf("/");
    if (slash <= 0) continue;
    const sid = key.slice(0, slash);
    const ch = Number(key.slice(slash + 1));
    if (!Number.isInteger(ch)) continue;
    client?.send({ type: "sub", sid, ch, payload: {} });
  }
}

function sendSourceListRequest(): void {
  client?.send({ type: "ctl", payload: { action: "list" } });
}

function handleConnState(state: ConnState): void {
  setConn(state);
  if (state.status === "open") {
    sendSourceListRequest();
    resubscribeChannels();
  }
}

export function getClient(): WireClient {
  if (!client) {
    const tokenEnv = import.meta.env.VITE_WANLOGGER_TOKEN;
    client = new WireClient({
      url: resolveWanloggerUrl(),
      ...(tokenEnv !== undefined ? { token: tokenEnv } : {}),
    });
    client.onState(handleConnState);
    client.onFrame(handleFrame);
    client.connect();
  }
  return client;
}

function handleFrame(frame: Frame): void {
  recordUiPerf((p) => {
    p.framesTotal += 1;
    p.lastFrameTsMs = Date.now();
  });
  if (frame.type === "data") {
    recordUiPerf((p) => {
      p.dataFrames += 1;
    });
    const p = frame.payload as DataPayload;
    const key = keyOf(p.sid, p.ch);
    const ls = channelListeners.get(key);
    if (ls) {
      recordUiPerf((perf) => {
        perf.subscriptionDispatches += ls.size;
      });
      for (const fn of ls) fn(p);
    }

    // Light-weight per-source aggregate.
    const existing = sources[p.sid];
    const size =
      p.body instanceof Uint8Array
        ? p.body.byteLength
        : Object.keys(p.body ?? {}).length;
    if (existing) {
      batch(() => {
        recordUiPerf((perf) => {
          perf.sourceUpdates += 1;
        });
        setSources(p.sid, "status", "running");
        setSources(p.sid, "lastTsMs", Number(p.ts_ingest) / 1_000_000);
        setSources(p.sid, "bytesIn", (bytesIn) => bytesIn + size);
        if (!existing.channels.includes(p.ch)) {
          setSources(p.sid, "channels", (channels) => [...channels, p.ch]);
        }
      });
    } else {
      recordUiPerf((perf) => {
        perf.sourceUpdates += 1;
      });
      setSources(p.sid, {
        sid: p.sid,
        name: p.source ?? p.sid.slice(0, 8),
        kind: p.kind,
        status: "running",
        channels: [p.ch],
        lastTsMs: Number(p.ts_ingest) / 1_000_000,
        bytesIn: size,
      });
    }
    return;
  }
  if (frame.type === "ctl") {
    recordUiPerf((p) => {
      p.ctlFrames += 1;
    });
    const p = frame.payload as Partial<CtlPayload>;
    const evt = p.event ?? "";
    if (evt === "sources") {
      syncSources(p.sources ?? []);
    } else if (evt === "error" || evt === "auth_failed" || evt === "ratelimited") {
      pushToast({
        level: "error",
        message: p.message ?? evt,
        ...(p.error_id ? { errorId: p.error_id } : {}),
      });
    } else if (evt === "write_ack") {
      // Acknowledgements can arrive for every terminal keystroke, so keep
      // them silent here. Explicit send-box feedback is handled locally by
      // the Terminal panel.
    } else if (evt === "disconnected" || evt === "eof") {
      if (p.sid) upsertSourceStatus(p.sid, "stopped");
      pushToast({ level: "warn", message: p.message ?? evt });
    } else if (
      evt === "started" ||
      evt === "resumed" ||
      evt === "restarted"
    ) {
      if (p.sid) upsertSourceStatus(p.sid, "running");
      sendSourceListRequest();
      const suffix = p.sid ? `: ${p.sid.slice(0, 8)}` : "";
      pushToast({ level: "info", message: p.message ?? `${evt}${suffix}` });
    } else if (evt === "stopped") {
      if (p.sid) upsertSourceStatus(p.sid, "stopped");
      sendSourceListRequest();
      const suffix = p.sid ? `: ${p.sid.slice(0, 8)}` : "";
      pushToast({ level: "info", message: p.message ?? `${evt}${suffix}` });
    } else if (evt === "removed") {
      if (p.sid) removeSource(p.sid);
      sendSourceListRequest();
      const suffix = p.sid ? `: ${p.sid.slice(0, 8)}` : "";
      pushToast({ level: "info", message: p.message ?? `${evt}${suffix}` });
    }
    return;
  }
  if (frame.type === "metrics") {
    recordUiPerf((p) => {
      p.metricsFrames += 1;
    });
    setMetrics(frame.payload as MetricsPayload);
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
  queueUiPerfPublish();
  return () => {
    const s = channelListeners.get(key);
    if (!s) return;
    s.delete(cb);
    if (s.size === 0) {
      channelListeners.delete(key);
      client?.send({ type: "unsub", sid, ch, payload: {} });
    }
    queueUiPerfPublish();
  };
}

/** Select the terminal panel's active subscription target. */
export function selectTerminalChannel(sid: string, ch: number): void {
  setTerminalChannelState({ sid, ch });
}

/** Select a terminal channel and request that the terminal panel is focused. */
export function openTerminalChannel(sid: string, ch: number): void {
  selectTerminalChannel(sid, ch);
  setTerminalFocusRequestState({ id: terminalFocusSeq++, sid, ch });
}

/** Send a control frame (e.g. start/stop a source). */
export function sendCtl(
  sid: string | undefined,
  action: "list" | "start" | "stop" | "resume" | "restart" | "remove",
  spec?: Record<string, unknown>,
): void {
  const payload: {
    action: "list" | "start" | "stop" | "resume" | "restart" | "remove";
    spec?: Record<string, unknown>;
  } = {
    action,
  };
  if (spec) payload.spec = spec;
  getClient().send({ type: "ctl", ...(sid ? { sid } : {}), payload });
}

/** Request a source list sync from the server. */
export function requestSourceList(): void {
  getClient().send({ type: "ctl", payload: { action: "list" } });
}

/** Send raw bytes to a (sid, ch). Used by terminal panel for TX. */
export function sendWrite(sid: string, ch: number, body: Uint8Array): void {
  getClient().send({ type: "write", sid, ch, payload: { body } });
}

export const connState = conn;
export const sourcesStore = sources;
export const metricsState = metrics;
export const uiPerfState = uiPerf;
export const terminalChannel = terminalChannelState;
export const terminalFocusRequest = terminalFocusRequestState;

/**
 * Test-only entry point: feed a frame through the same handler the
 * `WireClient` uses, without opening a WebSocket. Not part of the
 * public API.
 */
export function __ingestFrameForTest(frame: Frame): void {
  handleFrame(frame);
}

/** Test-only hook: drive connection-state side effects. */
export function __setConnStateForTest(state: ConnState): void {
  handleConnState(state);
}

/** Test-only hook: replace the wire client without opening a WebSocket. */
export function __setClientForTest(next: Pick<WireClient, "send"> | null): void {
  client = next as WireClient | null;
  channelListeners.clear();
  setTerminalChannelState(null);
  setTerminalFocusRequestState(null);
  terminalFocusSeq = 1;
}

/** Test-only hook: publish and read the latest local UI performance snapshot. */
export function __flushUiPerfForTest(): UiPerfSnapshot {
  publishUiPerf();
  return uiPerfSnapshot();
}
