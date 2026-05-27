import { parseSourceSpec } from "~/state/sourceSpec";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export interface SourcePreset {
  name: string;
  spec: string;
}

export const SOURCE_PRESETS_STORAGE_KEY = "wanlogger.sourcePresets.v1";

export const BUILTIN_SOURCE_PRESETS: SourcePreset[] = [
  { name: "mock demo", spec: "mock://demo" },
  { name: "tcp localhost", spec: "tcp://127.0.0.1:5555" },
  { name: "udp loopback", spec: "udp://127.0.0.1:0" },
  { name: "serial COM3", spec: "serial://COM3?baud=115200&data=8&parity=none&stop=1&flow=none" },
  { name: "file follow", spec: "file:///C:/logs/app.log?follow=1" },
];

export function isValidPresetName(name: string): boolean {
  return /^[A-Za-z0-9_.-]+$/.test(name);
}

function normalizePresetName(name: string): string {
  return name.trim();
}

function normalizeSpec(spec: string): string {
  return spec.trim();
}

function parseStoredPresets(raw: string | null): SourcePreset[] {
  if (!raw) return [];
  try {
    const value = JSON.parse(raw) as unknown;
    if (!Array.isArray(value)) return [];
    return value
      .filter((item): item is SourcePreset => {
        if (!item || typeof item !== "object") return false;
        const p = item as Partial<SourcePreset>;
        return typeof p.name === "string" && typeof p.spec === "string";
      })
      .map((p) => ({ name: normalizePresetName(p.name), spec: normalizeSpec(p.spec) }))
      .filter((p) => isValidPresetName(p.name) && p.spec.length > 0)
      .sort((a, b) => a.name.localeCompare(b.name));
  } catch {
    return [];
  }
}

export function loadUserSourcePresets(storage: StorageLike | undefined = browserStorage()): SourcePreset[] {
  return parseStoredPresets(safeGetItem(SOURCE_PRESETS_STORAGE_KEY, storage));
}

export function saveUserSourcePreset(
  name: string,
  spec: string,
  storage: StorageLike | undefined = browserStorage(),
): SourcePreset[] {
  const normalizedName = normalizePresetName(name);
  const normalizedSpec = normalizeSpec(spec);
  if (!isValidPresetName(normalizedName)) {
    throw new Error("preset name must use letters, numbers, dot, dash, or underscore");
  }
  if (!normalizedSpec) throw new Error("source spec is required");
  parseSourceSpec(normalizedSpec);

  const next = [
    ...loadUserSourcePresets(storage).filter((p) => p.name !== normalizedName),
    { name: normalizedName, spec: normalizedSpec },
  ].sort((a, b) => a.name.localeCompare(b.name));
  safeSetItem(SOURCE_PRESETS_STORAGE_KEY, JSON.stringify(next), storage);
  return next;
}

export function deleteUserSourcePreset(
  name: string,
  storage: StorageLike | undefined = browserStorage(),
): SourcePreset[] {
  const normalizedName = normalizePresetName(name);
  const next = loadUserSourcePresets(storage).filter((p) => p.name !== normalizedName);
  safeSetItem(SOURCE_PRESETS_STORAGE_KEY, JSON.stringify(next), storage);
  return next;
}
