import { describe, expect, it } from "vitest";
import type { SourceInfo } from "../../src/state";
import { filterAndSortSources } from "../../src/state/sourceFilters";

function source(overrides: Partial<SourceInfo>): SourceInfo {
  return {
    sid: "sid-default",
    name: "default",
    kind: "mock",
    status: "unknown",
    channels: [0],
    lastTsMs: 0,
    bytesIn: 0,
    ...overrides,
  };
}

describe("filterAndSortSources", () => {
  it("filters by query across source fields", () => {
    // REQ: FR-UI-008
    const rows = filterAndSortSources(
      [
        source({ sid: "sid-a", name: "UART A", kind: "serial", channels: [1] }),
        source({ sid: "sid-b", name: "TCP B", kind: "tcp", channels: [2] }),
      ],
      "serial",
      "all",
      "name",
    );

    expect(rows.map((s) => s.sid)).toEqual(["sid-a"]);
  });

  it("filters by lifecycle status", () => {
    // REQ: FR-UI-008
    const rows = filterAndSortSources(
      [
        source({ sid: "running", status: "running" }),
        source({ sid: "stopped", status: "stopped" }),
      ],
      "",
      "stopped",
      "name",
    );

    expect(rows.map((s) => s.sid)).toEqual(["stopped"]);
  });

  it("sorts by bytes descending", () => {
    // REQ: FR-UI-008
    const rows = filterAndSortSources(
      [
        source({ sid: "small", name: "small", bytesIn: 5 }),
        source({ sid: "large", name: "large", bytesIn: 50 }),
      ],
      "",
      "all",
      "bytes",
    );

    expect(rows.map((s) => s.sid)).toEqual(["large", "small"]);
  });
});
