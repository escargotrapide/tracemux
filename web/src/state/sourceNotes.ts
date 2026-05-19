import { createStore } from "solid-js/store";

export const SOURCE_NOTES_STORAGE_KEY = "wanlogger.sourceNotes.v1";
export const MAX_SOURCE_NOTE_LENGTH = 20_000;

export interface SourceNote {
  sid: string;
  text: string;
  updatedAt: number;
}

export type SourceNotes = Record<string, SourceNote>;

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

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

export function loadSourceNotes(storage = defaultStorage()): SourceNotes {
  const raw = storage?.getItem(SOURCE_NOTES_STORAGE_KEY) ?? null;
  if (!raw) return {};
  try {
    return normalizeSourceNotes(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveSourceNotes(notes: SourceNotes, storage = defaultStorage()): SourceNotes {
  const normalized = normalizeSourceNotes(notes);
  storage?.setItem(SOURCE_NOTES_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [sourceNotesStore, setSourceNotesStore] = createStore<SourceNotes>(loadSourceNotes());

export const sourceNotes = sourceNotesStore;

export function updateSourceNote(
  sid: string,
  text: string,
  storage = defaultStorage(),
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

export function deleteSourceNote(sid: string, storage = defaultStorage()): void {
  setSourceNotesStore(sid, undefined as unknown as SourceNote);
  const next = { ...sourceNotesStore };
  delete next[sid];
  saveSourceNotes(next, storage);
}
