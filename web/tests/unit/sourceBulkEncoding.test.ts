import { describe, expect, it } from "vitest";
import type { SourceInfo } from "../../src/state";
import {
  isBulkEncodingEligible,
  partitionBulkEncodingTargets,
} from "../../src/state/sourceFilters";

function source(overrides: Partial<SourceInfo>): SourceInfo {
  return {
    sid: "sid-default",
    name: "default",
    kind: "mock",
    status: "running",
    channels: [0],
    lastTsMs: 0,
    bytesIn: 0,
    decoder: "utf8-text:utf-8",
    ...overrides,
  };
}

describe("isBulkEncodingEligible", () => {
  it("accepts running text-decoder sources", () => {
    // REQ: FR-UI-014
    expect(isBulkEncodingEligible(source({ decoder: "utf8-text:shift_jis" }))).toBe(true);
  });

  it("rejects non-running sources", () => {
    // REQ: FR-UI-014
    expect(isBulkEncodingEligible(source({ status: "stopped" }))).toBe(false);
    expect(isBulkEncodingEligible(source({ status: "unknown" }))).toBe(false);
  });

  it("rejects non-text decoders", () => {
    // REQ: FR-UI-014
    expect(isBulkEncodingEligible(source({ decoder: "pcap-datagram" }))).toBe(false);
  });

  it("falls back to kind when the decoder is unknown", () => {
    // REQ: FR-UI-014
    expect(isBulkEncodingEligible(source({ decoder: undefined, kind: "serial" }))).toBe(true);
    expect(isBulkEncodingEligible(source({ decoder: undefined, kind: "pcap" }))).toBe(false);
  });
});

describe("partitionBulkEncodingTargets", () => {
  it("splits running sources into eligible and skipped, ignoring non-running", () => {
    // REQ: FR-UI-014
    const sources = [
      source({ sid: "text-a", decoder: "utf8-text:utf-8" }),
      source({ sid: "text-b", decoder: undefined, kind: "tcp" }),
      source({ sid: "pcap", decoder: "pcap-datagram", kind: "pcap" }),
      source({ sid: "stopped", status: "stopped", decoder: "utf8-text:utf-8" }),
    ];

    const { eligible, skipped } = partitionBulkEncodingTargets(sources);

    expect(eligible.map((s) => s.sid)).toEqual(["text-a", "text-b"]);
    expect(skipped.map((s) => s.sid)).toEqual(["pcap"]);
  });

  it("returns empty partitions when nothing is running", () => {
    // REQ: FR-UI-014
    const { eligible, skipped } = partitionBulkEncodingTargets([
      source({ sid: "stopped", status: "stopped" }),
    ]);

    expect(eligible).toEqual([]);
    expect(skipped).toEqual([]);
  });
});
