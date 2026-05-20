import { createStore } from "solid-js/store";

export const EXPORT_SETTINGS_STORAGE_KEY = "wanlogger.exportSettings.v1";

export interface ExportSettings {
  filenamePattern: string;
}

export const DEFAULT_EXPORT_SETTINGS: ExportSettings = {
  filenamePattern: "",
};

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

function stringOrEmpty(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.trim().slice(0, maxLength) : "";
}

export function normalizeExportSettings(value: unknown): ExportSettings {
  const input = value && typeof value === "object" ? value as Partial<ExportSettings> : {};
  return {
    filenamePattern: stringOrEmpty(input.filenamePattern, 240),
  };
}

export function loadExportSettings(storage = defaultStorage()): ExportSettings {
  const raw = storage?.getItem(EXPORT_SETTINGS_STORAGE_KEY) ?? null;
  if (!raw) return { ...DEFAULT_EXPORT_SETTINGS };
  try {
    return normalizeExportSettings(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_EXPORT_SETTINGS };
  }
}

export function saveExportSettings(
  settings: ExportSettings,
  storage = defaultStorage(),
): ExportSettings {
  const normalized = normalizeExportSettings(settings);
  storage?.setItem(EXPORT_SETTINGS_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [exportSettingsStore, setExportSettingsStore] = createStore<ExportSettings>(
  loadExportSettings(),
);

export const exportSettings = exportSettingsStore;

export function updateExportSettings(
  patch: Partial<ExportSettings>,
  storage = defaultStorage(),
): ExportSettings {
  const next = normalizeExportSettings({ ...exportSettingsStore, ...patch });
  setExportSettingsStore(next);
  saveExportSettings(next, storage);
  return next;
}