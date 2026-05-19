import { createStore } from "solid-js/store";

export const LOG_TYPE_NOTES_STORAGE_KEY = "wanlogger.logTypeNotes.v1";
export const MAX_LOG_TYPE_NOTE_LENGTH = 20_000;
export const MAX_LOG_TYPE_KEY_LENGTH = 120;

export interface LogTypeNote {
  key: string;
  text: string;
  updatedAt: number;
}

export type LogTypeNotes = Record<string, LogTypeNote>;

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

export function normalizeLogTypeKey(key: string): string {
  return key.trim().slice(0, MAX_LOG_TYPE_KEY_LENGTH);
}

function normalizeNote(value: unknown, fallbackKey: string): LogTypeNote | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<LogTypeNote>;
  const key = normalizeLogTypeKey(
    typeof input.key === "string" && input.key.trim() ? input.key : fallbackKey,
  );
  if (!key) return null;
  const text = typeof input.text === "string" ? input.text.slice(0, MAX_LOG_TYPE_NOTE_LENGTH) : "";
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return { key, text, updatedAt };
}

export function normalizeLogTypeNotes(value: unknown): LogTypeNotes {
  if (!value || typeof value !== "object") return {};
  const out: LogTypeNotes = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    const note = normalizeNote(raw, key);
    if (note) out[note.key] = note;
  }
  return out;
}

export function loadLogTypeNotes(storage = defaultStorage()): LogTypeNotes {
  const raw = storage?.getItem(LOG_TYPE_NOTES_STORAGE_KEY) ?? null;
  if (!raw) return {};
  try {
    return normalizeLogTypeNotes(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveLogTypeNotes(
  notes: LogTypeNotes,
  storage = defaultStorage(),
): LogTypeNotes {
  const normalized = normalizeLogTypeNotes(notes);
  storage?.setItem(LOG_TYPE_NOTES_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [logTypeNotesStore, setLogTypeNotesStore] = createStore<LogTypeNotes>(
  loadLogTypeNotes(),
);

export const logTypeNotes = logTypeNotesStore;

export function updateLogTypeNote(
  key: string,
  text: string,
  storage = defaultStorage(),
  now = Date.now(),
): LogTypeNote {
  const normalizedKey = normalizeLogTypeKey(key);
  const note = normalizeNote({ key: normalizedKey, text, updatedAt: now }, normalizedKey) ?? {
    key: normalizedKey,
    text: "",
    updatedAt: now,
  };
  setLogTypeNotesStore(note.key, note);
  saveLogTypeNotes({ ...logTypeNotesStore, [note.key]: note }, storage);
  return note;
}

export function deleteLogTypeNote(key: string, storage = defaultStorage()): void {
  const normalizedKey = normalizeLogTypeKey(key);
  if (!normalizedKey) return;
  setLogTypeNotesStore(normalizedKey, undefined as unknown as LogTypeNote);
  const next = { ...logTypeNotesStore };
  delete next[normalizedKey];
  saveLogTypeNotes(next, storage);
}
