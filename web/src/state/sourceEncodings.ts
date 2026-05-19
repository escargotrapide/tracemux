import { createStore } from "solid-js/store";
import { DEFAULT_SOURCE_ENCODING, normalizeEncoding } from "~/state/sourceStartOptions";

export const SOURCE_ENCODINGS_STORAGE_KEY = "wanlogger.sourceEncodings.v1";

export interface SourceEncoding {
  sid: string;
  encoding: string;
  updatedAt: number;
}

export type SourceEncodings = Record<string, SourceEncoding>;

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

function normalizeRecord(value: unknown, fallbackSid: string): SourceEncoding | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<SourceEncoding>;
  const sid = typeof input.sid === "string" && input.sid.trim() ? input.sid.trim() : fallbackSid;
  if (!sid) return null;
  const encoding = normalizeEncoding(input.encoding);
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return { sid, encoding, updatedAt };
}

export function normalizeSourceEncodings(value: unknown): SourceEncodings {
  if (!value || typeof value !== "object") return {};
  const out: SourceEncodings = {};
  for (const [sid, raw] of Object.entries(value as Record<string, unknown>)) {
    const record = normalizeRecord(raw, sid);
    if (record && record.encoding !== DEFAULT_SOURCE_ENCODING) out[record.sid] = record;
  }
  return out;
}

export function loadSourceEncodings(storage = defaultStorage()): SourceEncodings {
  const raw = storage?.getItem(SOURCE_ENCODINGS_STORAGE_KEY) ?? null;
  if (!raw) return {};
  try {
    return normalizeSourceEncodings(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveSourceEncodings(
  encodings: SourceEncodings,
  storage = defaultStorage(),
): SourceEncodings {
  const normalized = normalizeSourceEncodings(encodings);
  storage?.setItem(SOURCE_ENCODINGS_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [sourceEncodingsStore, setSourceEncodingsStore] = createStore<SourceEncodings>(
  loadSourceEncodings(),
);

export const sourceEncodings = sourceEncodingsStore;

export function encodingForSource(sid: string): string {
  return sourceEncodingsStore[sid]?.encoding ?? DEFAULT_SOURCE_ENCODING;
}

export function updateSourceEncoding(
  sid: string,
  encoding: string,
  storage = defaultStorage(),
  now = Date.now(),
): SourceEncoding | null {
  const record = normalizeRecord({ sid, encoding, updatedAt: now }, sid);
  if (!record || record.encoding === DEFAULT_SOURCE_ENCODING) {
    deleteSourceEncoding(sid, storage);
    return null;
  }
  setSourceEncodingsStore(record.sid, record);
  saveSourceEncodings({ ...sourceEncodingsStore, [record.sid]: record }, storage);
  return record;
}

export function deleteSourceEncoding(sid: string, storage = defaultStorage()): void {
  setSourceEncodingsStore(sid, undefined as unknown as SourceEncoding);
  const next = { ...sourceEncodingsStore };
  delete next[sid];
  saveSourceEncodings(next, storage);
}
