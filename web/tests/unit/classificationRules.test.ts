import { describe, expect, it } from "vitest";
import {
  CLASSIFICATION_RULES_STORAGE_KEY,
  MAX_CLASSIFICATION_TEXT_LENGTH,
  classifyText,
  loadClassificationRules,
  normalizeClassificationRules,
  saveClassificationRules,
  upsertClassificationRule,
  wireClassificationRules,
} from "../../src/state/classificationRules";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("classification rules", () => {
  it("normalizes and filters malformed rules", () => {
    // REQ: FR-UI-011
    const rules = normalizeClassificationRules({
      fault: { contains: " ERROR ", tag: " fault ", caseSensitive: true, updatedAt: 2 },
      empty: { contains: "", tag: "skip" },
      disabled: { contains: "warn", tag: "warning", enabled: false },
    });

    expect(rules.fault).toMatchObject({
      contains: "ERROR",
      tag: "fault",
      caseSensitive: true,
      enabled: true,
    });
    expect(rules.disabled).toMatchObject({ enabled: false });
    expect(rules.empty).toBeUndefined();
  });

  it("saves, loads, truncates, and classifies text", () => {
    const storage = new FakeStorage();
    const longText = "x".repeat(MAX_CLASSIFICATION_TEXT_LENGTH + 3);
    const rule = upsertClassificationRule(
      { contains: longText, tag: "fault", enabled: true },
      storage,
      9,
    );

    expect(rule.contains).toHaveLength(MAX_CLASSIFICATION_TEXT_LENGTH);
    expect(storage.getItem(CLASSIFICATION_RULES_STORAGE_KEY)).toContain("fault");
    expect(loadClassificationRules(storage)[rule.id]?.updatedAt).toBe(9);
    expect(classifyText(`prefix ${rule.contains} suffix`, [rule])).toEqual(["fault"]);

    const saved = saveClassificationRules(
      { warn: { id: "warn", contains: "warn", tag: "warning", caseSensitive: false, enabled: true, updatedAt: 1 } },
      storage,
    );
    expect(loadClassificationRules(storage)).toEqual(saved);
  });

  it("deduplicates wire tags and honors case sensitivity", () => {
    const rules = [
      { id: "a", contains: "ERR", tag: "fault", caseSensitive: true, enabled: true, updatedAt: 1 },
      { id: "b", contains: "err", tag: "fault", caseSensitive: false, enabled: true, updatedAt: 2 },
    ];

    expect(classifyText("err", rules)).toEqual(["fault"]);
    expect(wireClassificationRules(rules)).toEqual([
      { contains: "ERR", tag: "fault", case_sensitive: true },
      { contains: "err", tag: "fault" },
    ]);
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(CLASSIFICATION_RULES_STORAGE_KEY, "{bad json");

    expect(loadClassificationRules(storage)).toEqual({});
  });
});
