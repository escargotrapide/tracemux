import { describe, expect, it } from "vitest";
import {
  SOURCE_ENCODINGS_STORAGE_KEY,
  encodingForSource,
  loadSourceEncodings,
  normalizeSourceEncodings,
  saveSourceEncodings,
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
        sid2: { encoding: "utf-8" },
        bad: null,
      }),
    ).toEqual({
      sid1: { sid: "sid1", encoding: "shift_jis", updatedAt: 3 },
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

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_ENCODINGS_STORAGE_KEY, "{bad json");

    expect(loadSourceEncodings(storage)).toEqual({});
  });
});
