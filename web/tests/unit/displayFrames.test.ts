import { describe, expect, it } from "vitest";
import {
  bodyText,
  clientClassificationTags,
  labelForSid,
  metadataPrefix,
  payloadMatchesFilter,
  renderPayload,
  sourceDisplayName,
  type DisplayFilter,
} from "../../src/state/displayFrames";
import type { DataPayload } from "../../src/adapters/wss";

function payload(patch: Partial<DataPayload> = {}): DataPayload {
  return {
    ts_origin: 0,
    ts_ingest: 1,
    mono_ns: 0,
    boot_id: "b",
    node_id: "n",
    clock_offset_ms: 0,
    clock_quality: "best-effort",
    drift_ppm: 0,
    clock_source: "system",
    sid: "sid-123456",
    ch: 0,
    dir: "in",
    kind: "bytes",
    body: new Uint8Array([72, 73]),
    tags: ["fault"],
    source: "serial:COM7",
    ...patch,
  };
}

describe("display frame helpers", () => {
  it("prefers aliases and source labels over short ids", () => {
    // REQ: FR-UI-014
    expect(labelForSid("sid-123456", { "sid-123456": { name: "COM7" } })).toBe("COM7");
    expect(
      sourceDisplayName(
        { sid: "sid-123456", source: "serial:COM7" },
        { "sid-123456": { name: "COM7" } },
        { "sid-123456": { label: "Motor UART" } },
      ),
    ).toBe("Motor UART");
  });

  it("builds metadata prefixes from display settings", () => {
    // REQ: FR-UI-014
    const prefix = metadataPrefix(
      payload(),
      { showTimestamp: true, showKind: true, showSource: true, timezone: "UTC" },
      "COM7",
    );

    expect(prefix).toContain("UTC");
    expect(prefix).toContain("bytes:fault");
    expect(prefix).toContain("COM7");
  });

  it("renders byte and object payloads", () => {
    const bytes = renderPayload(
      payload(),
      { showTimestamp: false, showKind: true, showSource: false, timezone: "local" },
      "COM7",
    );
    expect(bytes).toEqual({ text: "[bytes:fault] HI", newline: false });

    const object = renderPayload(
      payload({ body: { ok: true }, kind: "record" }),
      { showTimestamp: false, showKind: false, showSource: false, timezone: "local" },
      "COM7",
    );
    expect(object).toEqual({ text: "{\"ok\":true}", newline: true });
    expect(bodyText(payload())).toBe("HI");
  });

  it("filters by kind, tags, and source", () => {
    // REQ: FR-UI-011
    const filter: DisplayFilter = {
      kind: "bytes",
      tagQuery: "fault",
      sourceQuery: "com7",
    };

    expect(payloadMatchesFilter(payload(), filter, "serial:COM7")).toBe(true);
    expect(payloadMatchesFilter(payload({ kind: "record" }), filter, "serial:COM7")).toBe(false);
    expect(payloadMatchesFilter(payload(), { ...filter, tagQuery: "warn" }, "serial:COM7")).toBe(false);
    expect(payloadMatchesFilter(payload(), { ...filter, sourceQuery: "com8" }, "serial:COM7")).toBe(false);
  });

  it("decodes bytes with selected encoding and filters client-side tags", () => {
    // REQ: FR-UI-011
    const p = payload({ body: new Uint8Array([0x82, 0xa0]) });
    const rules = [
      { id: "jp", contains: "あ", tag: "jp-text", caseSensitive: false, enabled: true, updatedAt: 1 },
    ];
    const tags = clientClassificationTags(p, rules, "shift_jis");

    expect(bodyText(p, "shift_jis")).toBe("あ");
    expect(tags).toEqual(["jp-text"]);
    expect(payloadMatchesFilter(p, { kind: "all", tagQuery: "jp", sourceQuery: "" }, "COM7", tags)).toBe(true);
    expect(renderPayload(
      p,
      { showTimestamp: false, showKind: true, showSource: false, timezone: "local" },
      "COM7",
      { encoding: "shift_jis", extraTags: tags },
    )).toEqual({ text: "[bytes:fault|jp-text] あ", newline: false });
  });
});
