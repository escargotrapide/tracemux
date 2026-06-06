import { describe, expect, it } from "vitest";
import {
  DEFAULT_DISPLAY_SETTINGS,
  DISPLAY_SETTINGS_STORAGE_KEY,
  formatTimestampNs,
  isValidDisplayTimezone,
  loadDisplaySettings,
  normalizeDisplaySettings,
  resetDisplaySettings,
  saveDisplaySettings,
  updateDisplaySettings,
} from "../../src/state/displaySettings";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("display settings", () => {
  it("normalizes untrusted stored values", () => {
    // REQ: FR-UI-014
    expect(
      normalizeDisplaySettings({
        terminalScrollback: -1,
        tileScrollback: "250",
        terminalMaxRecords: 9,
        tileMaxRecords: 999_999,
        tileMinWidth: 999_999,
        tileMinHeight: 10,
        showTimestamp: true,
        showKind: "nope",
        showSource: true,
        timezone: "GMT+09:00",
      }),
    ).toEqual({
      ...DEFAULT_DISPLAY_SETTINGS,
      terminalScrollback: 100,
      tileScrollback: 250,
      terminalMaxRecords: 100,
      tileMaxRecords: 100_000,
      tileMinWidth: 1200,
      tileMinHeight: 80,
      showTimestamp: true,
      showSource: true,
      timezone: "GMT+09:00",
    });
  });

  it("coerces the tile rendering pause flag", () => {
    // REQ: FR-UI-012
    expect(normalizeDisplaySettings({ tileRenderingPaused: true }).tileRenderingPaused).toBe(true);
    expect(normalizeDisplaySettings({ tileRenderingPaused: "yes" }).tileRenderingPaused).toBe(
      false,
    );
    expect(DEFAULT_DISPLAY_SETTINGS.tileRenderingPaused).toBe(false);
  });

  it("loads defaults for malformed storage", () => {
    // REQ: FR-UI-014
    const storage = new FakeStorage();
    storage.setItem(DISPLAY_SETTINGS_STORAGE_KEY, "{not json");

    expect(loadDisplaySettings(storage)).toEqual(DEFAULT_DISPLAY_SETTINGS);
  });

  it("saves and updates settings", () => {
    // REQ: FR-UI-014
    const storage = new FakeStorage();
    const saved = saveDisplaySettings(
      { ...DEFAULT_DISPLAY_SETTINGS, terminalScrollback: 1234 },
      storage,
    );

    expect(loadDisplaySettings(storage)).toEqual(saved);

    const updated = updateDisplaySettings({ tileMinWidth: 333 }, storage);
    expect(updated.tileMinWidth).toBe(333);
    expect(loadDisplaySettings(storage).tileMinWidth).toBe(333);

    const reset = resetDisplaySettings(storage);
    expect(reset).toEqual(DEFAULT_DISPLAY_SETTINGS);
    expect(loadDisplaySettings(storage)).toEqual(DEFAULT_DISPLAY_SETTINGS);
  });

  it("validates supported time zone inputs", () => {
    // REQ: FR-UI-014
    expect(isValidDisplayTimezone("local")).toBe(true);
    expect(isValidDisplayTimezone("UTC")).toBe(true);
    expect(isValidDisplayTimezone("Asia/Tokyo")).toBe(true);
    expect(isValidDisplayTimezone("GMT+09:00")).toBe(true);
    expect(isValidDisplayTimezone("Not/AZone")).toBe(false);
  });

  it("formats UTC and GMT offset timestamps", () => {
    // REQ: FR-UI-014
    expect(formatTimestampNs(0, { timezone: "UTC" })).toContain("UTC");
    expect(formatTimestampNs(0, { timezone: "GMT+9" })).toMatch(
      /1970-01-01.*09:00:00.*GMT\+9/,
    );
    expect(formatTimestampNs(0, { timezone: "GMT+09:00" })).toMatch(
      /1970-01-01.*09:00:00.*GMT\+09:00/,
    );
    expect(formatTimestampNs(0, { timezone: "+09:00" })).toMatch(
      /1970-01-01.*09:00:00.*\+09:00/,
    );
  });
});
