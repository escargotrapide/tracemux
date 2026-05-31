import { describe, expect, it, afterEach } from "vitest";
import {
  __resetChannelBuffersForTest,
  appendChannelFrame,
  clearChannelFrames,
  getChannelFrames,
} from "../../src/state/channelBuffers";
import type { DataPayload } from "../../src/adapters/wss";

function payload(seq: number, sid = "sid", ch = 0): DataPayload {
  return {
    ts_origin: seq,
    ts_ingest: seq,
    mono_ns: 0,
    boot_id: "b",
    node_id: "n",
    clock_offset_ms: 0,
    clock_quality: "best-effort",
    drift_ppm: 0,
    clock_source: "system",
    sid,
    ch,
    dir: "in",
    kind: "bytes",
    body: new Uint8Array([seq]),
  };
}

describe("channel buffers", () => {
  afterEach(() => __resetChannelBuffersForTest());

  it("keeps only the latest records up to the configured cap", () => {
    // REQ: FR-UI-014
    appendChannelFrame(payload(1), 2);
    appendChannelFrame(payload(2), 2);
    appendChannelFrame(payload(3), 2);

    expect(getChannelFrames("sid", 0).map((p) => p.ts_origin)).toEqual([2, 3]);
    expect(getChannelFrames("sid", 0, 1).map((p) => p.ts_origin)).toEqual([3]);
  });

  it("separates channels and can clear per source", () => {
    appendChannelFrame(payload(1, "a", 0), 10);
    appendChannelFrame(payload(2, "a", 1), 10);
    appendChannelFrame(payload(3, "b", 0), 10);

    clearChannelFrames("a");

    expect(getChannelFrames("a", 0)).toEqual([]);
    expect(getChannelFrames("a", 1)).toEqual([]);
    expect(getChannelFrames("b", 0).map((p) => p.ts_origin)).toEqual([3]);
  });
});
