import { describe, expect, it } from "vitest";
import {
  MAX_SOURCE_NOTE_LENGTH,
  SOURCE_NOTES_STORAGE_KEY,
  loadSourceNotes,
  normalizeSourceNotes,
  saveSourceNotes,
  updateSourceNote,
} from "../../src/state/sourceNotes";

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

describe("source notes", () => {
  it("normalizes note records", () => {
    // REQ: FR-UI-017
    expect(
      normalizeSourceNotes({
        sid1: { text: "memo", updatedAt: 123 },
        sid2: { sid: "custom", text: "x", updatedAt: -1 },
        bad: null,
      }),
    ).toEqual({
      sid1: { sid: "sid1", text: "memo", updatedAt: 123 },
      custom: { sid: "custom", text: "x", updatedAt: 0 },
    });
  });

  it("saves and loads notes from storage", () => {
    // REQ: FR-UI-017
    const storage = new FakeStorage();
    const saved = saveSourceNotes(
      { sid1: { sid: "sid1", text: "operator memo", updatedAt: 42 } },
      storage,
    );

    expect(storage.getItem(SOURCE_NOTES_STORAGE_KEY)).toContain("operator memo");
    expect(loadSourceNotes(storage)).toEqual(saved);
  });

  it("truncates overly long notes", () => {
    const storage = new FakeStorage();
    const note = updateSourceNote("sid1", "x".repeat(MAX_SOURCE_NOTE_LENGTH + 5), storage, 7);

    expect(note.text).toHaveLength(MAX_SOURCE_NOTE_LENGTH);
    expect(loadSourceNotes(storage).sid1?.updatedAt).toBe(7);
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(SOURCE_NOTES_STORAGE_KEY, "{not json");

    expect(loadSourceNotes(storage)).toEqual({});
  });
});
