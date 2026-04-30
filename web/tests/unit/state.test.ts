import { afterEach, describe, expect, it, vi } from "vitest";
import {
  __flushUiPerfForTest,
  __ingestFrameForTest,
  __setClientForTest,
  __setConnStateForTest,
  metricsState,
  uiPerfState,
  terminalChannel,
  terminalFocusRequest,
  sourcesStore,
  toastsStore,
  pushToast,
  dismissToast,
  openTerminalChannel,
  selectTerminalChannel,
  requestSourceList,
  sendCtl,
  sendWrite,
  useChannel,
} from "../../src/state";

describe("state frame handler", () => {
  afterEach(() => {
    __setClientForTest(null);
  });

  it("updates sourcesStore on data frames", () => {
    // REQ: FR-UI-003
    const before = __flushUiPerfForTest();
    __ingestFrameForTest({
      type: "data",
      seq: 1,
      payload: {
        ts_origin: 0,
        ts_ingest: 1_000_000,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "s1",
        ch: 0,
        dir: "in",
        kind: "bytes",
        body: new Uint8Array([1, 2, 3]),
        source: "uart0",
      },
    });
    expect(sourcesStore.s1).toBeDefined();
    expect(sourcesStore.s1.bytesIn).toBe(3);
    expect(sourcesStore.s1.status).toBe("running");
    expect(sourcesStore.s1.channels).toEqual([0]);
    const after = __flushUiPerfForTest();
    expect(after.framesTotal).toBe(before.framesTotal + 1);
    expect(after.dataFrames).toBe(before.dataFrames + 1);
    expect(after.sourceUpdates).toBe(before.sourceUpdates + 1);
    expect(uiPerfState().dataFrames).toBe(after.dataFrames);
  });

  it("batches existing source aggregate updates", () => {
    // REQ: FR-UI-003
    __ingestFrameForTest({
      type: "data",
      seq: 11,
      payload: {
        ts_origin: 0,
        ts_ingest: 1_000_000,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "s-perf",
        ch: 0,
        dir: "in",
        kind: "bytes",
        body: new Uint8Array([1, 2, 3]),
      },
    });
    __ingestFrameForTest({
      type: "data",
      seq: 12,
      payload: {
        ts_origin: 0,
        ts_ingest: 2_000_000,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "s-perf",
        ch: 1,
        dir: "in",
        kind: "bytes",
        body: new Uint8Array([4, 5]),
      },
    });

    expect(sourcesStore["s-perf"].bytesIn).toBe(5);
    expect(sourcesStore["s-perf"].lastTsMs).toBe(2);
    expect(sourcesStore["s-perf"].channels).toEqual([0, 1]);
  });

  it("sends terminal write frames", () => {
    // REQ: FR-UI-010
    const send = vi.fn();
    __setClientForTest({ send });
    const body = new Uint8Array([0x41, 0x42]);

    sendWrite("sid-tx", 2, body);

    expect(send).toHaveBeenCalledWith({
      type: "write",
      sid: "sid-tx",
      ch: 2,
      payload: { body },
    });
  });

  it("sends source lifecycle ctl frames", () => {
    // REQ: FR-UI-008
    const send = vi.fn();
    __setClientForTest({ send });

    sendCtl(undefined, "start", { kind: "mock", tag: "ui" });
    sendCtl("sid-stop", "stop");
    sendCtl("sid-restart", "restart");

    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      payload: { action: "start", spec: { kind: "mock", tag: "ui" } },
    });
    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      sid: "sid-stop",
      payload: { action: "stop" },
    });
    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      sid: "sid-restart",
      payload: { action: "restart" },
    });
  });

  it("requests source list sync", () => {
    // REQ: FR-UI-008
    const send = vi.fn();
    __setClientForTest({ send });

    requestSourceList();

    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      payload: { action: "list" },
    });
  });

  it("subscribes, routes, and unsubscribes terminal channels", () => {
    // REQ: FR-UI-011
    const send = vi.fn();
    __setClientForTest({ send });
    const seen: number[] = [];

    const unsubscribe = useChannel("sid-sub", 7, (p) => {
      if (p.body instanceof Uint8Array) seen.push(p.body[0] ?? -1);
    });

    expect(send).toHaveBeenCalledWith({
      type: "sub",
      sid: "sid-sub",
      ch: 7,
      payload: {},
    });
    expect(__flushUiPerfForTest().activeSubscriptions).toBeGreaterThanOrEqual(1);

    __ingestFrameForTest({
      type: "data",
      seq: 10,
      payload: {
        ts_origin: 0,
        ts_ingest: 1_000_000,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "sid-sub",
        ch: 7,
        dir: "in",
        kind: "bytes",
        body: new Uint8Array([9]),
      },
    });
    expect(seen).toEqual([9]);
    expect(__flushUiPerfForTest().subscriptionDispatches).toBeGreaterThanOrEqual(1);

    unsubscribe();
    expect(send).toHaveBeenCalledWith({
      type: "unsub",
      sid: "sid-sub",
      ch: 7,
      payload: {},
    });
  });

  it("replays source list and channel subscriptions on reconnect", () => {
    // REQ: FR-UI-011
    const send = vi.fn();
    __setClientForTest({ send });

    const unsubscribe = useChannel("sid-resub", 5, () => undefined);
    send.mockClear();

    __setConnStateForTest({ status: "open", since: 123 });

    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      payload: { action: "list" },
    });
    expect(send).toHaveBeenCalledWith({
      type: "sub",
      sid: "sid-resub",
      ch: 5,
      payload: {},
    });

    unsubscribe();
  });

  it("selects the active terminal channel", () => {
    // REQ: FR-UI-011
    selectTerminalChannel("sid-open", 3);

    expect(terminalChannel()).toEqual({ sid: "sid-open", ch: 3 });
  });

  it("requests terminal focus when opening a channel", () => {
    // REQ: FR-UI-011
    openTerminalChannel("sid-focus", 4);

    expect(terminalChannel()).toEqual({ sid: "sid-focus", ch: 4 });
    expect(terminalFocusRequest()).toEqual({ id: 1, sid: "sid-focus", ch: 4 });
  });

  it("captures the latest metrics frame", () => {
    // REQ: FR-UI-007
    __ingestFrameForTest({
      type: "metrics",
      seq: 2,
      payload: { records: { "s1/0": 42 } },
    });
    expect(metricsState()).toEqual({ records: { "s1/0": 42 } });
  });

  it("ctl error frames produce error toasts", () => {
    // REQ: FR-UI-009
    const before = toastsStore.length;
    __ingestFrameForTest({
      type: "ctl",
      seq: 3,
      payload: { event: "auth_failed", message: "bad token", error_id: "E-2001" },
    });
    expect(toastsStore.length).toBe(before + 1);
    const t = toastsStore[toastsStore.length - 1];
    expect(t.level).toBe("error");
    expect(t.errorId).toBe("E-2001");
  });

  it("ctl lifecycle ack frames produce info toasts", () => {
    // REQ: FR-UI-009
    const send = vi.fn();
    __setClientForTest({ send });
    const before = toastsStore.length;
    __ingestFrameForTest({
      type: "ctl",
      seq: 4,
      payload: { event: "restarted", sid: "12345678-aaaa", message: "source restarted" },
    });
    expect(toastsStore.length).toBe(before + 1);
    const t = toastsStore[toastsStore.length - 1];
    expect(t.level).toBe("info");
    expect(t.message).toBe("source restarted");
    expect(sourcesStore["12345678-aaaa"].status).toBe("running");
    expect(send).toHaveBeenCalledWith({
      type: "ctl",
      payload: { action: "list" },
    });
  });

  it("ctl stop and remove update source status", () => {
    // REQ: FR-UI-008
    __ingestFrameForTest({
      type: "ctl",
      seq: 5,
      payload: { event: "started", sid: "sid-status", message: "source started" },
    });
    expect(sourcesStore["sid-status"].status).toBe("running");

    __ingestFrameForTest({
      type: "ctl",
      seq: 6,
      payload: { event: "stopped", sid: "sid-status", message: "source stopped" },
    });
    expect(sourcesStore["sid-status"].status).toBe("stopped");

    __ingestFrameForTest({
      type: "ctl",
      seq: 7,
      payload: { event: "removed", sid: "sid-status", message: "source removed" },
    });
    expect(sourcesStore["sid-status"]).toBeUndefined();
  });

  it("ctl sources frames resync the source table", () => {
    // REQ: FR-UI-008
    __ingestFrameForTest({
      type: "ctl",
      seq: 8,
      payload: { event: "started", sid: "sid-stale", message: "source started" },
    });
    expect(sourcesStore["sid-stale"]).toBeDefined();

    __ingestFrameForTest({
      type: "ctl",
      seq: 9,
      payload: {
        event: "sources",
        sources: [
          {
            sid: "sid-sync",
            name: "sync source",
            kind: "mock",
            status: "running",
            channels: [0, 2],
            bytes_in: 12,
            last_ts_ms: 42,
          },
        ],
      },
    });

    expect(sourcesStore["sid-stale"]).toBeUndefined();
    expect(sourcesStore["sid-sync"]).toEqual({
      sid: "sid-sync",
      name: "sync source",
      kind: "mock",
      status: "running",
      channels: [0, 2],
      lastTsMs: 42,
      bytesIn: 12,
    });
  });

  it("pushToast / dismissToast round-trip", () => {
    const id = pushToast({ level: "info", message: "hi" });
    expect(toastsStore.find((t) => t.id === id)?.message).toBe("hi");
    dismissToast(id);
    expect(toastsStore.find((t) => t.id === id)).toBeUndefined();
  });

  it("caps toast history", () => {
    const before = __flushUiPerfForTest();
    for (let i = 0; i < 70; i += 1) {
      pushToast({ level: "info", message: `toast ${i}` });
    }

    expect(toastsStore.length).toBeLessThanOrEqual(64);
    const after = __flushUiPerfForTest();
    expect(after.toastsPushed).toBe(before.toastsPushed + 70);
    expect(after.toastsDropped).toBeGreaterThan(before.toastsDropped);
  });
});
