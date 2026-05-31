// Browser-local display encoding overrides for source/channel rendering.
//
// REQ: FR-UI-014

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { DEFAULT_SOURCE_ENCODING, normalizeEncoding } from "~/state/sourceStartOptions";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const SOURCE_ENCODINGS_STORAGE_KEY = "tracemux.sourceEncodings.v1";

export interface SourceEncoding {
  sid: string;
  ch?: number;
  encoding: string;
  updatedAt: number;
}

export type SourceEncodings = Record<string, SourceEncoding>;

function validChannel(value: unknown): number | undefined {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) return undefined;
  return parsed;
}

export function sourceEncodingKey(sid: string): string {
  return sid.trim();
}

export function channelEncodingKey(sid: string, ch: number): string {
  return `${sourceEncodingKey(sid)}/${Math.max(0, Math.trunc(ch))}`;
}

function keyParts(key: string): Pick<SourceEncoding, "sid" | "ch"> {
  const slash = key.lastIndexOf("/");
  if (slash <= 0) return { sid: key.trim() };
  const sid = key.slice(0, slash).trim();
  const ch = validChannel(key.slice(slash + 1));
  return ch === undefined ? { sid } : { sid, ch };
}

function recordKey(record: Pick<SourceEncoding, "sid" | "ch">): string {
  return record.ch === undefined
    ? sourceEncodingKey(record.sid)
    : channelEncodingKey(record.sid, record.ch);
}

function normalizeRecord(value: unknown, fallbackKey: string): SourceEncoding | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<SourceEncoding>;
  const fallback = keyParts(fallbackKey);
  const sid = typeof input.sid === "string" && input.sid.trim() ? input.sid.trim() : fallback.sid;
  if (!sid) return null;
  const ch = input.ch === undefined ? fallback.ch : validChannel(input.ch);
  const encoding = normalizeEncoding(input.encoding);
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return ch === undefined ? { sid, encoding, updatedAt } : { sid, ch, encoding, updatedAt };
}

export function normalizeSourceEncodings(value: unknown): SourceEncodings {
  if (!value || typeof value !== "object") return {};
  const out: SourceEncodings = {};
  for (const [sid, raw] of Object.entries(value as Record<string, unknown>)) {
    const record = normalizeRecord(raw, sid);
    if (record) out[recordKey(record)] = record;
  }
  return out;
}

export function loadSourceEncodings(storage = browserStorage()): SourceEncodings {
  const raw = safeGetItem(SOURCE_ENCODINGS_STORAGE_KEY, storage);
  if (!raw) return {};
  try {
    return normalizeSourceEncodings(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveSourceEncodings(
  encodings: SourceEncodings,
  storage = browserStorage(),
): SourceEncodings {
  const normalized = normalizeSourceEncodings(encodings);
  safeSetItem(SOURCE_ENCODINGS_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [sourceEncodingsStore, setSourceEncodingsStore] = createStore<SourceEncodings>(
  loadSourceEncodings(),
);
const [sourceEncodingsVersionState, setSourceEncodingsVersionState] = createSignal(0);

export const sourceEncodings = sourceEncodingsStore;
export const sourceEncodingsVersion = sourceEncodingsVersionState;

function bumpSourceEncodingsVersion(): void {
  setSourceEncodingsVersionState((version) => version + 1);
}

export function encodingForSource(sid: string, fallback = DEFAULT_SOURCE_ENCODING): string {
  return sourceEncodingsStore[sourceEncodingKey(sid)]?.encoding ?? fallback;
}

export function encodingForChannel(
  sid: string,
  ch: number,
  fallback = DEFAULT_SOURCE_ENCODING,
): string {
  return sourceEncodingsStore[channelEncodingKey(sid, ch)]?.encoding
    ?? encodingForSource(sid, fallback);
}

export function updateSourceEncoding(
  sid: string,
  encoding: string,
  storage = browserStorage(),
  now = Date.now(),
  inheritedEncoding = DEFAULT_SOURCE_ENCODING,
): SourceEncoding | null {
  const record = normalizeRecord({ sid, encoding, updatedAt: now }, sid);
  if (!record || record.encoding === normalizeEncoding(inheritedEncoding)) {
    deleteSourceEncoding(sid, storage);
    return null;
  }
  const key = recordKey(record);
  setSourceEncodingsStore(key, record);
  saveSourceEncodings({ ...sourceEncodingsStore, [key]: record }, storage);
  bumpSourceEncodingsVersion();
  return record;
}

export function updateChannelEncoding(
  sid: string,
  ch: number,
  encoding: string,
  storage = browserStorage(),
  now = Date.now(),
  inheritedEncoding = encodingForSource(sid),
): SourceEncoding | null {
  const key = channelEncodingKey(sid, ch);
  const record = normalizeRecord({ sid, ch, encoding, updatedAt: now }, key);
  if (!record || record.encoding === normalizeEncoding(inheritedEncoding)) {
    deleteChannelEncoding(sid, ch, storage);
    return null;
  }
  setSourceEncodingsStore(key, record);
  saveSourceEncodings({ ...sourceEncodingsStore, [key]: record }, storage);
  bumpSourceEncodingsVersion();
  return record;
}

export function deleteSourceEncoding(sid: string, storage = browserStorage()): void {
  const key = sourceEncodingKey(sid);
  setSourceEncodingsStore(key, undefined as unknown as SourceEncoding);
  const next = { ...sourceEncodingsStore };
  delete next[key];
  saveSourceEncodings(next, storage);
  bumpSourceEncodingsVersion();
}

export function deleteChannelEncoding(
  sid: string,
  ch: number,
  storage = browserStorage(),
): void {
  const key = channelEncodingKey(sid, ch);
  setSourceEncodingsStore(key, undefined as unknown as SourceEncoding);
  const next = { ...sourceEncodingsStore };
  delete next[key];
  saveSourceEncodings(next, storage);
  bumpSourceEncodingsVersion();
}
