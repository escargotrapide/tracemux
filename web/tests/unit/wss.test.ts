import { afterEach, describe, expect, it, vi } from "vitest";
import { Packr } from "msgpackr";
import { WireClient, resolveTraceMuxHttpUrl, resolveTraceMuxUrl } from "../../src/adapters/wss";

const packr = new Packr({ useRecords: false, mapsAsObjects: true });

class FakeWebSocket extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  binaryType: BinaryType = "blob";
  readyState = FakeWebSocket.CONNECTING;
  sent: ArrayBuffer[] = [];

  constructor(
    readonly url: string,
    readonly protocols?: string | string[],
  ) {
    super();
    FakeWebSocket.instances.push(this);
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    if (this.readyState !== FakeWebSocket.OPEN) {
      throw new Error("socket is not open");
    }
    this.sent.push(data as ArrayBuffer);
  }

  close(): void {
    this.readyState = FakeWebSocket.CLOSED;
  }

  open(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.dispatchEvent(new Event("open"));
  }

  receive(data: ArrayBuffer): void {
    const event = new Event("message") as MessageEvent;
    Object.defineProperty(event, "data", { value: data });
    this.dispatchEvent(event);
  }
}

function stubLocation(location: Partial<Location>): void {
  vi.stubGlobal("window", {
    location: {
      protocol: "http:",
      hostname: "127.0.0.1",
      host: "127.0.0.1:5173",
      port: "5173",
      ...location,
    },
  });
}

function packed(value: unknown): ArrayBuffer {
  const data = packr.pack(value) as Uint8Array;
  const buffer = new ArrayBuffer(data.byteLength);
  new Uint8Array(buffer).set(data);
  return buffer;
}

describe("resolveTraceMuxUrl", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    FakeWebSocket.instances = [];
  });

  it("honors VITE_TRACEMUX_URL", () => {
    vi.stubEnv("VITE_TRACEMUX_URL", "ws://example.test/ws");
    expect(resolveTraceMuxUrl()).toBe("ws://example.test/ws");
  });

  it("uses the loopback backend for the Vite dev server", () => {
    stubLocation({
      protocol: "http:",
      hostname: "127.0.0.1",
      host: "127.0.0.1:5173",
      port: "5173",
    });
    expect(resolveTraceMuxUrl()).toBe("ws://127.0.0.1:9000/ws");
    expect(resolveTraceMuxHttpUrl("/api/detect")).toBe(
      "http://127.0.0.1:9000/api/detect",
    );
  });

  it("uses the page host for deployed HTTP origins", () => {
    stubLocation({
      protocol: "https:",
      hostname: "logs.example.test",
      host: "logs.example.test",
      port: "",
    });
    expect(resolveTraceMuxUrl()).toBe("wss://logs.example.test/ws");
    expect(resolveTraceMuxHttpUrl("/api/version")).toBe(
      "https://logs.example.test/api/version",
    );
  });

  it("uses the loopback backend for Tauri custom protocols", () => {
    stubLocation({
      protocol: "tauri:",
      hostname: "localhost",
      host: "localhost",
      port: "",
    });
    expect(resolveTraceMuxUrl()).toBe("ws://127.0.0.1:9000/ws");
  });

  it("reports whether send actually reached an open socket", () => {
    // REQ: FR-UI-009
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const client = new WireClient({ url: "ws://example.test/ws" });

    client.connect();
    const ws = FakeWebSocket.instances[0];
    expect(ws).toBeDefined();
    expect(client.send({ type: "ping", payload: {} })).toBe(false);

    ws?.open();
    expect(client.send({ type: "ping", payload: {} })).toBe(true);
    expect(ws?.sent.length).toBe(2);
  });

  it("emits protocol errors for malformed MessagePack frames", () => {
    // REQ: FR-UI-009
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const client = new WireClient({ url: "ws://example.test/ws" });
    const errors: string[] = [];
    client.onError((err) => errors.push(err.errorId));

    client.connect();
    const ws = FakeWebSocket.instances[0];
    ws?.open();
    ws?.receive(new Uint8Array([0x81]).buffer);

    expect(errors).toContain("E-UI-0010");
  });

  it("emits protocol errors for unsupported frame types", () => {
    // REQ: FR-UI-009
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const client = new WireClient({ url: "ws://example.test/ws" });
    const errors: string[] = [];
    const frames: string[] = [];
    client.onError((err) => errors.push(`${err.errorId}:${err.message}`));
    client.onFrame((frame) => frames.push(frame.type));

    client.connect();
    const ws = FakeWebSocket.instances[0];
    ws?.open();
    ws?.receive(packed({ type: "future_frame", seq: 1, payload: {} }));

    expect(errors).toContain("E-UI-0010:Unsupported WSS frame ignored");
    expect(frames).toEqual([]);
  });

  it("rejects known frame types with malformed envelopes", () => {
    // REQ: FR-UI-009
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const client = new WireClient({ url: "ws://example.test/ws" });
    const errors: string[] = [];
    const frames: string[] = [];
    client.onError((err) => errors.push(`${err.errorId}:${err.message}`));
    client.onFrame((frame) => frames.push(frame.type));

    client.connect();
    const ws = FakeWebSocket.instances[0];
    ws?.open();
    ws?.receive(packed({ type: "metrics", seq: 1 }));

    expect(errors).toContain("E-UI-0010:Malformed WSS frame ignored");
    expect(frames).toEqual([]);
  });
});
