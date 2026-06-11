import { describe, expect, it } from "vitest";
import {
  DEFAULT_SOURCE_ENCODING,
  MONITOR_WINDOW_SECONDS_LIMITS,
  SOURCE_START_OPTIONS_STORAGE_KEY,
  loadSourceStartOptions,
  normalizeDetectionMode,
  normalizeMonitorWindowSeconds,
  normalizeEncoding,
  normalizeSourceStartOptions,
  resetSourceStartOptions,
  saveSourceStartOptions,
  startCtlOptions,
  updateSourceStartOptions,
} from "../../src/state/sourceStartOptions";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("source start options", () => {
  it("normalizes encoding and start defaults", () => {
    // REQ: FR-UI-008
    expect(normalizeEncoding(" Shift_JIS ")).toBe("shift_jis");
    expect(normalizeEncoding("  ")).toBe(DEFAULT_SOURCE_ENCODING);
    expect(normalizeDetectionMode("AUTO")).toBe("auto");
    expect(normalizeDetectionMode("nope")).toBe("configured");
    expect(
      normalizeSourceStartOptions({
        encoding: "CP932",
        detectionMode: "suggest",
        sessionNamePattern: " {prefix}-{kind} ",
        sendClassificationRules: false,
      }),
    ).toEqual({
      encoding: "cp932",
      detectionMode: "suggest",
      monitorWindowSeconds: 30,
      sessionNamePattern: "{prefix}-{kind}",
      sendClassificationRules: false,
    });
  });

  it("normalizes and clamps the monitor window", () => {
    expect(normalizeDetectionMode("MONITOR")).toBe("monitor");
    expect(normalizeMonitorWindowSeconds("45")).toBe(45);
    expect(normalizeMonitorWindowSeconds(0)).toBe(MONITOR_WINDOW_SECONDS_LIMITS.min);
    expect(normalizeMonitorWindowSeconds(99999)).toBe(MONITOR_WINDOW_SECONDS_LIMITS.max);
    expect(normalizeMonitorWindowSeconds("nope")).toBe(30);
  });

  it("includes monitor window in ctl options only for monitor mode", () => {
    expect(
      startCtlOptions({
        encoding: "utf-8",
        detectionMode: "monitor",
        monitorWindowSeconds: 12,
        sessionNamePattern: "",
        sendClassificationRules: false,
      }),
    ).toEqual({
      encoding: "utf-8",
      detection_mode: "monitor",
      monitor_window_secs: 12,
    });
    expect(
      startCtlOptions({
        encoding: "utf-8",
        detectionMode: "auto",
        monitorWindowSeconds: 12,
        sessionNamePattern: "",
        sendClassificationRules: false,
      }),
    ).toEqual({
      encoding: "utf-8",
      detection_mode: "auto",
    });
  });

  it("saves and loads options", () => {
    const storage = new FakeStorage();
    const options = updateSourceStartOptions(
      { encoding: "shift_jis", sessionNamePattern: "{kind}-{iface}" },
      storage,
    );

    expect(storage.getItem(SOURCE_START_OPTIONS_STORAGE_KEY)).toContain("shift_jis");
    expect(loadSourceStartOptions(storage)).toEqual(options);

    const saved = saveSourceStartOptions(
      { encoding: "utf-8", detectionMode: "configured", sessionNamePattern: "", sendClassificationRules: true },
      storage,
    );
    expect(loadSourceStartOptions(storage)).toEqual(saved);

    const reset = resetSourceStartOptions(storage);
    expect(reset.encoding).toBe(DEFAULT_SOURCE_ENCODING);
    expect(loadSourceStartOptions(storage)).toEqual(reset);
  });

  it("builds start ctl options", () => {
    expect(
      startCtlOptions({
        encoding: "shift_jis",
        detectionMode: "auto",
        sessionNamePattern: "{prefix}-{kind}",
        sendClassificationRules: false,
      }),
    ).toEqual({
      encoding: "shift_jis",
      detection_mode: "auto",
      session_name_pattern: "{prefix}-{kind}",
    });
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_START_OPTIONS_STORAGE_KEY, "{bad json");

    expect(loadSourceStartOptions(storage).encoding).toBe(DEFAULT_SOURCE_ENCODING);
  });
});
