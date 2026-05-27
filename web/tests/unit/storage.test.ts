import { describe, expect, it, vi } from "vitest";
import { safeGetItem, safeSetItem } from "../../src/state/storage";
import {
  DEFAULT_DISPLAY_SETTINGS,
  loadDisplaySettings,
  saveDisplaySettings,
} from "../../src/state/displaySettings";

describe("safe browser storage", () => {
  it("treats storage get/set exceptions as unavailable storage", () => {
    // REQ: FR-UI-018
    const storage = {
      getItem: vi.fn(() => {
        throw new Error("blocked");
      }),
      setItem: vi.fn(() => {
        throw new Error("quota");
      }),
    };

    expect(safeGetItem("k", storage)).toBeNull();
    expect(safeSetItem("k", "v", storage)).toBe(false);
    expect(loadDisplaySettings(storage)).toEqual(DEFAULT_DISPLAY_SETTINGS);
    expect(saveDisplaySettings({ ...DEFAULT_DISPLAY_SETTINGS, timezone: "UTC" }, storage)).toEqual({
      ...DEFAULT_DISPLAY_SETTINGS,
      timezone: "UTC",
    });
  });
});