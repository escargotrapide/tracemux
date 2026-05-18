// WSS adapter for the wanlogger.v1 wire protocol.
//
// Spec: docs/protocols/wire-protocol.md
// - Subprotocol: `wanlogger.v1` + `bearer.<token>`
// - Binary frames are MessagePack maps with fields:
//     { type, sid?, ch?, seq, payload }
//
// This adapter only knows how to serialize/deserialize. Higher-level
// state lives in `~/state/`.

import { Packr, Unpackr } from "msgpackr";

export type FrameType =
  | "hello"
  | "auth"
  | "sub"
  | "unsub"
  | "data"
  | "ctl"
  | "write"
  | "metrics"
  | "clientlog"
  | "ping"
  | "pong"
  | "clock_sync"
  | "panel_priority";

export interface Frame<P = unknown> {
  type: FrameType;
  sid?: string;
  ch?: number;
  seq: number;
  payload: P;
}

export interface DataPayload {
  ts_origin: bigint | number;
  ts_ingest: bigint | number;
  mono_ns: bigint | number;
  boot_id: string;
  node_id: string;
  clock_offset_ms: number;
  clock_quality: "synced" | "best-effort" | "unknown" | "imported";
  drift_ppm: number;
  clock_source: "system" | "ntp" | "ptp" | "monotonic" | "imported";
  sid: string;
  ch: number;
  dir: "in" | "out";
  kind: "bytes" | "datagram" | "frame" | "record";
  body: Uint8Array | Record<string, unknown>;
  level?: "trace" | "debug" | "info" | "warn" | "error" | "fatal";
  tags?: string[];
  correlation_id?: string;
  source?: string;
  host?: string;
  schema_id?: string;
}

export interface MetricsPayload {
  /** ISO timestamp or epoch ms; opaque on the wire. */
  ts?: string | number;
  /** Per-source byte counters keyed by `sid`. */
  bytes_in?: Record<string, number>;
  /** Per-channel record counters keyed by `sid/ch`. */
  records?: Record<string, number>;
  /** Free-form additional gauges. */
  [key: string]: unknown;
}

export interface CtlPayload {
  event:
    | "sources"
    | "started"
    | "stopped"
    | "resumed"
    | "restarted"
    | "removed"
    | "connected"
    | "disconnected"
    | "eof"
    | "write_ack"
    | "error"
    | "ratelimited"
    | "auth_failed";
  sid?: string;
  ch?: number;
  bytes_written?: number;
  sources?: SourceSyncPayload[];
  message?: string;
  error_id?: string;
}

export interface SourceSyncPayload {
  sid: string;
  name?: string;
  kind?: string;
  status?: "running" | "stopped" | "unknown";
  channels?: number[];
  bytes_in?: number;
  last_ts_ms?: number;
}

export type ConnState =
  | { status: "idle" }
  | { status: "connecting" }
  | { status: "open"; since: number }
  | { status: "closed"; code: number; reason: string }
  | { status: "error"; message: string };

export interface WireClientOptions {
  url: string;
  token?: string;
  /** Reconnect backoff base in ms (default 500). */
  backoffMs?: number;
  /** Cap for backoff (default 15_000). */
  backoffMaxMs?: number;
}

const SUBPROTO = "wanlogger.v1";
const DEFAULT_DEV_URL = "ws://127.0.0.1:9000/ws";
const VITE_DEV_PORT = "5173";

const packr = new Packr({ useRecords: false, mapsAsObjects: true });
const unpackr = new Unpackr({ useRecords: false, mapsAsObjects: true });

export type FrameListener = (frame: Frame) => void;
export type StateListener = (state: ConnState) => void;

export class WireClient {
  private ws: WebSocket | null = null;
  private seqOut = 0n;
  private state: ConnState = { status: "idle" };
  private frameListeners = new Set<FrameListener>();
  private stateListeners = new Set<StateListener>();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private attempt = 0;
  private closedByUser = false;

  constructor(private readonly opts: WireClientOptions) {}

  get connectionState(): ConnState {
    return this.state;
  }

  onFrame(fn: FrameListener): () => void {
    this.frameListeners.add(fn);
    return () => this.frameListeners.delete(fn);
  }

  onState(fn: StateListener): () => void {
    this.stateListeners.add(fn);
    fn(this.state);
    return () => this.stateListeners.delete(fn);
  }

  connect(): void {
    if (this.ws && this.ws.readyState <= WebSocket.OPEN) {
      return;
    }
    this.closedByUser = false;
    this.setState({ status: "connecting" });

    const protocols: string[] = [SUBPROTO];
    if (this.opts.token) {
      protocols.push(`bearer.${this.opts.token}`);
    }

    let ws: WebSocket;
    try {
      ws = new WebSocket(this.opts.url, protocols);
    } catch (err) {
      this.setState({
        status: "error",
        message: (err as Error).message ?? "WebSocket construction failed",
      });
      this.scheduleReconnect();
      return;
    }
    ws.binaryType = "arraybuffer";
    this.ws = ws;

    ws.addEventListener("open", () => {
      this.attempt = 0;
      this.setState({ status: "open", since: Date.now() });
      this.send({
        type: "hello",
        seq: 0,
        payload: { app: "wanlogger-web", version: "0.1.0-dev" },
      });
    });

    ws.addEventListener("message", (ev) => {
      const data = ev.data;
      if (!(data instanceof ArrayBuffer)) {
        // text frames are not used for v1 binary protocol
        return;
      }
      try {
        const frame = unpackr.unpack(new Uint8Array(data)) as Frame;
        for (const fn of this.frameListeners) {
          fn(frame);
        }
      } catch (err) {
        // swallow malformed frames; server is the source of truth
        console.warn("E-UI-0010 unpack failed", err);
      }
    });

    ws.addEventListener("close", (ev) => {
      this.ws = null;
      this.setState({
        status: "closed",
        code: ev.code,
        reason: ev.reason || "",
      });
      if (!this.closedByUser) {
        this.scheduleReconnect();
      }
    });

    ws.addEventListener("error", () => {
      // 'close' will fire afterwards; mark error first so UI can show it.
      if (this.state.status !== "closed") {
        this.setState({ status: "error", message: "WebSocket error" });
      }
    });
  }

  close(): void {
    this.closedByUser = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.ws?.close(1000, "client closing");
    this.ws = null;
  }

  send<P>(frame: Omit<Frame<P>, "seq"> & { seq?: number }): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      return;
    }
    const seq = frame.seq ?? Number(this.seqOut++ & 0xffff_ffff_ffff_ffffn);
    const out: Frame<P> = { ...frame, seq } as Frame<P>;
    const packed = packr.pack(out) as Uint8Array;
    const buf = new ArrayBuffer(packed.byteLength);
    new Uint8Array(buf).set(packed);
    this.ws.send(buf);
  }

  private setState(s: ConnState): void {
    this.state = s;
    for (const fn of this.stateListeners) fn(s);
  }

  private scheduleReconnect(): void {
    if (this.closedByUser) return;
    const base = this.opts.backoffMs ?? 500;
    const max = this.opts.backoffMaxMs ?? 15_000;
    const delay = Math.min(max, base * 2 ** this.attempt);
    this.attempt += 1;
    this.reconnectTimer = setTimeout(() => this.connect(), delay);
  }
}

/** Resolve the WebSocket endpoint to use, honouring env overrides. */
export function resolveWanloggerUrl(): string {
  const fromEnv = import.meta.env.VITE_WANLOGGER_URL?.trim();
  if (fromEnv && fromEnv.length > 0) return fromEnv;

  if (typeof window !== "undefined") {
    const { protocol, hostname, host, port } = window.location;
    const isViteDevHost =
      port === VITE_DEV_PORT &&
      (hostname === "127.0.0.1" || hostname === "localhost");
    if (isViteDevHost) return DEFAULT_DEV_URL;

    if (protocol === "http:" || protocol === "https:") {
      const proto = protocol === "https:" ? "wss:" : "ws:";
      return `${proto}//${host}/ws`;
    }
  }

  // Tauri/custom protocols do not have an HTTP origin to reuse.
  return DEFAULT_DEV_URL;
}

/** Resolve a server HTTP API URL matching the configured WSS endpoint. */
export function resolveWanloggerHttpUrl(path: string): string {
  try {
    const url = new URL(resolveWanloggerUrl());
    url.protocol = url.protocol === "wss:" ? "https:" : "http:";
    url.pathname = path.startsWith("/") ? path : `/${path}`;
    url.search = "";
    url.hash = "";
    return url.toString();
  } catch {
    return path;
  }
}
