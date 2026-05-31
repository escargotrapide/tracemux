import { describe, expect, it } from "vitest";
import {
  BUILTIN_SOURCE_PRESETS,
  SOURCE_PRESETS_STORAGE_KEY,
  deleteUserSourcePreset,
  isValidPresetName,
  loadUserSourcePresets,
  saveUserSourcePreset,
} from "../../src/state/sourcePresets";

class FakeStorage implements Storage {
  private readonly data = new Map<string, string>();

  get length(): number {
    return this.data.size;
  }

  clear(): void {
    this.data.clear();
  }

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.data.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.data.delete(key);
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("source presets", () => {
  it("ships built-in presets", () => {
    // REQ: FR-UI-008
    expect(BUILTIN_SOURCE_PRESETS.some((p) => p.spec === "mock://demo")).toBe(true);
  });

  it("saves, replaces, sorts, and deletes browser profiles", () => {
    // REQ: FR-UI-008
    const storage = new FakeStorage();

    let presets = saveUserSourcePreset("lab-b", "tcp://127.0.0.1:5555", storage);
    presets = saveUserSourcePreset("lab-a", "mock://demo", storage);
    expect(presets.map((p) => p.name)).toEqual(["lab-a", "lab-b"]);

    presets = saveUserSourcePreset("lab-a", "udp://127.0.0.1:0", storage);
    expect(presets).toEqual([
      { name: "lab-a", spec: "udp://127.0.0.1:0" },
      { name: "lab-b", spec: "tcp://127.0.0.1:5555" },
    ]);
    expect(loadUserSourcePresets(storage)).toEqual(presets);

    expect(deleteUserSourcePreset("lab-b", storage)).toEqual([
      { name: "lab-a", spec: "udp://127.0.0.1:0" },
    ]);
  });

  it("validates names and source specs", () => {
    const storage = new FakeStorage();

    expect(isValidPresetName("lab_1.prod")).toBe(true);
    expect(isValidPresetName("../secret")).toBe(false);
    expect(() => saveUserSourcePreset("bad/name", "mock://demo", storage)).toThrow(/preset name/);
    expect(() => saveUserSourcePreset("good", "not-a-uri", storage)).toThrow(/missing scheme/);
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_PRESETS_STORAGE_KEY, "{not json");

    expect(loadUserSourcePresets(storage)).toEqual([]);
  });
});
