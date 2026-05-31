import { describe, expect, it } from "vitest";
import {
  SOURCE_ENCODINGS_STORAGE_KEY,
  channelEncodingKey,
  encodingForChannel,
  encodingForSource,
  loadSourceEncodings,
  normalizeSourceEncodings,
  saveSourceEncodings,
  updateChannelEncoding,
  updateSourceEncoding,
} from "../../src/state/sourceEncodings";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("source encodings", () => {
  it("normalizes per-source encoding records", () => {
    // REQ: FR-UI-014
    expect(
      normalizeSourceEncodings({
        sid1: { encoding: " Shift_JIS ", updatedAt: 3 },
        "sid1/1": { encoding: " euc-jp ", updatedAt: 4 },
        sid2: { encoding: "utf-8" },
        bad: null,
      }),
    ).toEqual({
      sid1: { sid: "sid1", encoding: "shift_jis", updatedAt: 3 },
      "sid1/1": { sid: "sid1", ch: 1, encoding: "euc-jp", updatedAt: 4 },
      sid2: { sid: "sid2", encoding: "utf-8", updatedAt: 0 },
    });
  });

  it("saves, loads, and clears utf-8 defaults", () => {
    const storage = new FakeStorage();
    const record = updateSourceEncoding("sid1", "cp932", storage, 7);

    expect(record?.encoding).toBe("cp932");
    expect(storage.getItem(SOURCE_ENCODINGS_STORAGE_KEY)).toContain("sid1");
    expect(loadSourceEncodings(storage).sid1?.updatedAt).toBe(7);
    expect(encodingForSource("unknown")).toBe("utf-8");

    expect(updateSourceEncoding("sid1", "utf-8", storage)).toBeNull();
    expect(loadSourceEncodings(storage).sid1).toBeUndefined();

    const saved = saveSourceEncodings(
      { sid2: { sid: "sid2", encoding: "euc-jp", updatedAt: 2 } },
      storage,
    );
    expect(loadSourceEncodings(storage)).toEqual(saved);
  });

  it("supports channel-level overrides with source fallback", () => {
    // REQ: FR-UI-014
    const storage = new FakeStorage();
    updateSourceEncoding("sid1", "cp932", storage, 10);
    const record = updateChannelEncoding("sid1", 1, "shift_jis", storage, 11);

    expect(record).toEqual({ sid: "sid1", ch: 1, encoding: "shift_jis", updatedAt: 11 });
    expect(loadSourceEncodings(storage)[channelEncodingKey("sid1", 1)]?.encoding).toBe("shift_jis");
    expect(encodingForSource("sid1")).toBe("cp932");
    expect(encodingForChannel("sid1", 0)).toBe("cp932");
    expect(encodingForChannel("sid1", 1)).toBe("shift_jis");

    expect(updateChannelEncoding("sid1", 1, "cp932", storage)).toBeNull();
    expect(loadSourceEncodings(storage)[channelEncodingKey("sid1", 1)]).toBeUndefined();
  });

  it("stores utf-8 as an override when inherited encoding is not utf-8", () => {
    const storage = new FakeStorage();
    const record = updateChannelEncoding("sid-inherited", 0, "utf-8", storage, 20, "cp932");

    expect(record).toEqual({ sid: "sid-inherited", ch: 0, encoding: "utf-8", updatedAt: 20 });
    expect(encodingForChannel("sid-inherited", 0, "cp932")).toBe("utf-8");
    expect(loadSourceEncodings(storage)[channelEncodingKey("sid-inherited", 0)]?.encoding).toBe("utf-8");

    expect(updateChannelEncoding("sid-inherited", 0, "cp932", storage, 21, "cp932")).toBeNull();
    expect(loadSourceEncodings(storage)[channelEncodingKey("sid-inherited", 0)]).toBeUndefined();
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_ENCODINGS_STORAGE_KEY, "{bad json");

    expect(loadSourceEncodings(storage)).toEqual({});
  });
});
