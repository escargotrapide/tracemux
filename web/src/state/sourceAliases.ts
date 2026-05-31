import { createStore } from "solid-js/store";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const SOURCE_ALIASES_STORAGE_KEY = "tracemux.sourceAliases.v1";
export const MAX_SOURCE_ALIAS_LENGTH = 80;

export interface SourceAlias {
  sid: string;
  label: string;
  updatedAt: number;
}

export type SourceAliases = Record<string, SourceAlias>;

function normalizeAlias(value: unknown, fallbackSid: string): SourceAlias | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<SourceAlias>;
  const sid = typeof input.sid === "string" && input.sid.trim() ? input.sid.trim() : fallbackSid;
  if (!sid.trim()) return null;
  const label = typeof input.label === "string"
    ? input.label.trim().slice(0, MAX_SOURCE_ALIAS_LENGTH)
    : "";
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return { sid, label, updatedAt };
}

export function normalizeSourceAliases(value: unknown): SourceAliases {
  if (!value || typeof value !== "object") return {};
  const out: SourceAliases = {};
  for (const [sid, raw] of Object.entries(value as Record<string, unknown>)) {
    const alias = normalizeAlias(raw, sid);
    if (alias && alias.label) out[alias.sid] = alias;
  }
  return out;
}

export function loadSourceAliases(storage = browserStorage()): SourceAliases {
  const raw = safeGetItem(SOURCE_ALIASES_STORAGE_KEY, storage);
  if (!raw) return {};
  try {
    return normalizeSourceAliases(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveSourceAliases(
  aliases: SourceAliases,
  storage = browserStorage(),
): SourceAliases {
  const normalized = normalizeSourceAliases(aliases);
  safeSetItem(SOURCE_ALIASES_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [sourceAliasesStore, setSourceAliasesStore] = createStore<SourceAliases>(
  loadSourceAliases(),
);

export const sourceAliases = sourceAliasesStore;

export function updateSourceAlias(
  sid: string,
  label: string,
  storage = browserStorage(),
  now = Date.now(),
): SourceAlias | null {
  const alias = normalizeAlias({ sid, label, updatedAt: now }, sid);
  if (!alias || !alias.label) {
    deleteSourceAlias(sid, storage);
    return null;
  }
  setSourceAliasesStore(alias.sid, alias);
  saveSourceAliases({ ...sourceAliasesStore, [alias.sid]: alias }, storage);
  return alias;
}

export function deleteSourceAlias(sid: string, storage = browserStorage()): void {
  setSourceAliasesStore(sid, undefined as unknown as SourceAlias);
  const next = { ...sourceAliasesStore };
  delete next[sid];
  saveSourceAliases(next, storage);
}
