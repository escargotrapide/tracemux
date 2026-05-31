import { describe, expect, it } from "vitest";
import {
  DEFAULT_EXPORT_SETTINGS,
  EXPORT_SETTINGS_STORAGE_KEY,
  loadExportSettings,
  normalizeExportSettings,
  saveExportSettings,
  updateExportSettings,
} from "../../src/state/exportSettings";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("export settings", () => {
  it("normalizes and persists filename patterns", () => {
    // REQ: FR-EXP-001
    const long = "x".repeat(300);
    expect(normalizeExportSettings({ filenamePattern: ` ${long} ` }).filenamePattern).toHaveLength(240);
    expect(normalizeExportSettings({ filenamePattern: 7 })).toEqual(DEFAULT_EXPORT_SETTINGS);

    const storage = new FakeStorage();
    const updated = updateExportSettings({ filenamePattern: "{source}_{timestamp}.{ext}" }, storage);
    expect(storage.getItem(EXPORT_SETTINGS_STORAGE_KEY)).toContain("{source}_{timestamp}.{ext}");
    expect(loadExportSettings(storage)).toEqual(updated);

    const saved = saveExportSettings({ filenamePattern: "{sid}.{ext}" }, storage);
    expect(loadExportSettings(storage)).toEqual(saved);
  });

  it("falls back to defaults for malformed storage", () => {
    const storage = new FakeStorage();
    storage.setItem(EXPORT_SETTINGS_STORAGE_KEY, "{bad json");

    expect(loadExportSettings(storage)).toEqual(DEFAULT_EXPORT_SETTINGS);
  });
});
