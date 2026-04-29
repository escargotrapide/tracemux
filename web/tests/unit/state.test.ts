import { describe, it, expect } from "vitest";
import {
  __ingestFrameForTest,
  metricsState,
  sourcesStore,
  toastsStore,
  pushToast,
  dismissToast,
} from "../../src/state";

describe("state frame handler", () => {
  it("updates sourcesStore on data frames", () => {
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
    expect(sourcesStore.s1.channels).toEqual([0]);
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

  it("pushToast / dismissToast round-trip", () => {
    const id = pushToast({ level: "info", message: "hi" });
    expect(toastsStore.find((t) => t.id === id)?.message).toBe("hi");
    dismissToast(id);
    expect(toastsStore.find((t) => t.id === id)).toBeUndefined();
  });
});
