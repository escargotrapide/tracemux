import { describe, expect, it } from "vitest";
import {
  MAX_SOURCE_ALIAS_LENGTH,
  SOURCE_ALIASES_STORAGE_KEY,
  loadSourceAliases,
  normalizeSourceAliases,
  saveSourceAliases,
  updateSourceAlias,
} from "../../src/state/sourceAliases";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("source aliases", () => {
  it("normalizes alias records", () => {
    // REQ: FR-UI-014
    expect(
      normalizeSourceAliases({
        sid1: { label: " COM7 ", updatedAt: 123 },
        sid2: { sid: "custom", label: "", updatedAt: 1 },
        bad: null,
      }),
    ).toEqual({
      sid1: { sid: "sid1", label: "COM7", updatedAt: 123 },
    });
  });

  it("saves, loads, and truncates aliases", () => {
    const storage = new FakeStorage();
    const alias = updateSourceAlias("sid1", "x".repeat(MAX_SOURCE_ALIAS_LENGTH + 5), storage, 7);

    expect(alias?.label).toHaveLength(MAX_SOURCE_ALIAS_LENGTH);
    expect(storage.getItem(SOURCE_ALIASES_STORAGE_KEY)).toContain("sid1");
    expect(loadSourceAliases(storage).sid1?.updatedAt).toBe(7);

    const saved = saveSourceAliases({ sid2: { sid: "sid2", label: "Pump", updatedAt: 2 } }, storage);
    expect(loadSourceAliases(storage)).toEqual(saved);
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_ALIASES_STORAGE_KEY, "{not json");

    expect(loadSourceAliases(storage)).toEqual({});
  });
});
