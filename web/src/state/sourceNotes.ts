import { createStore } from "solid-js/store";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const SOURCE_NOTES_STORAGE_KEY = "wanlogger.sourceNotes.v1";
export const MAX_SOURCE_NOTE_LENGTH = 20_000;

export interface SourceNote {
  sid: string;
  text: string;
  updatedAt: number;
}

export type SourceNotes = Record<string, SourceNote>;

function normalizeNote(value: unknown, fallbackSid: string): SourceNote | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<SourceNote>;
  const sid = typeof input.sid === "string" && input.sid.trim() ? input.sid.trim() : fallbackSid;
  if (!sid.trim()) return null;
  const text = typeof input.text === "string" ? input.text.slice(0, MAX_SOURCE_NOTE_LENGTH) : "";
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return { sid, text, updatedAt };
}

export function normalizeSourceNotes(value: unknown): SourceNotes {
  if (!value || typeof value !== "object") return {};
  const out: SourceNotes = {};
  for (const [sid, raw] of Object.entries(value as Record<string, unknown>)) {
    const note = normalizeNote(raw, sid);
    if (note) out[note.sid] = note;
  }
  return out;
}

export function loadSourceNotes(storage = browserStorage()): SourceNotes {
  const raw = safeGetItem(SOURCE_NOTES_STORAGE_KEY, storage);
  if (!raw) return {};
  try {
    return normalizeSourceNotes(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveSourceNotes(notes: SourceNotes, storage = browserStorage()): SourceNotes {
  const normalized = normalizeSourceNotes(notes);
  safeSetItem(SOURCE_NOTES_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [sourceNotesStore, setSourceNotesStore] = createStore<SourceNotes>(loadSourceNotes());

export const sourceNotes = sourceNotesStore;

export function updateSourceNote(
  sid: string,
  text: string,
  storage = browserStorage(),
  now = Date.now(),
): SourceNote {
  const note = normalizeNote({ sid, text, updatedAt: now }, sid) ?? {
    sid,
    text: "",
    updatedAt: now,
  };
  setSourceNotesStore(note.sid, note);
  saveSourceNotes({ ...sourceNotesStore, [note.sid]: note }, storage);
  return note;
}

export function deleteSourceNote(sid: string, storage = browserStorage()): void {
  setSourceNotesStore(sid, undefined as unknown as SourceNote);
  const next = { ...sourceNotesStore };
  delete next[sid];
  saveSourceNotes(next, storage);
}
