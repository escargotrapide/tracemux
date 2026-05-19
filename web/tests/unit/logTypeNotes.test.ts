import { describe, expect, it } from "vitest";
import {
  LOG_TYPE_NOTES_STORAGE_KEY,
  MAX_LOG_TYPE_NOTE_LENGTH,
  loadLogTypeNotes,
  normalizeLogTypeKey,
  normalizeLogTypeNotes,
  saveLogTypeNotes,
  updateLogTypeNote,
} from "../../src/state/logTypeNotes";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

describe("log type notes", () => {
  it("normalizes keys and note records", () => {
    // REQ: FR-UI-017
    expect(normalizeLogTypeKey(" fault ")).toBe("fault");
    expect(
      normalizeLogTypeNotes({
        fault: { text: "check motor", updatedAt: 9 },
        bytes: { key: "raw", text: "raw bytes", updatedAt: -1 },
        bad: null,
      }),
    ).toEqual({
      fault: { key: "fault", text: "check motor", updatedAt: 9 },
      raw: { key: "raw", text: "raw bytes", updatedAt: 0 },
    });
  });

  it("saves, loads, and truncates notes", () => {
    const storage = new FakeStorage();
    const note = updateLogTypeNote("fault", "x".repeat(MAX_LOG_TYPE_NOTE_LENGTH + 5), storage, 7);

    expect(note.text).toHaveLength(MAX_LOG_TYPE_NOTE_LENGTH);
    expect(storage.getItem(LOG_TYPE_NOTES_STORAGE_KEY)).toContain("fault");
    expect(loadLogTypeNotes(storage).fault?.updatedAt).toBe(7);

    const saved = saveLogTypeNotes({ warn: { key: "warn", text: "watch", updatedAt: 2 } }, storage);
    expect(loadLogTypeNotes(storage)).toEqual(saved);
  });

  it("ignores malformed stored data", () => {
    const storage = new FakeStorage();
    storage.setItem(LOG_TYPE_NOTES_STORAGE_KEY, "{not json");

    expect(loadLogTypeNotes(storage)).toEqual({});
  });
});
