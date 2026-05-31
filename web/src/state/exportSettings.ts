import { createStore } from "solid-js/store";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const EXPORT_SETTINGS_STORAGE_KEY = "tracemux.exportSettings.v1";

export interface ExportSettings {
  filenamePattern: string;
}

export const DEFAULT_EXPORT_SETTINGS: ExportSettings = {
  filenamePattern: "",
};

function stringOrEmpty(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.trim().slice(0, maxLength) : "";
}

export function normalizeExportSettings(value: unknown): ExportSettings {
  const input = value && typeof value === "object" ? value as Partial<ExportSettings> : {};
  return {
    filenamePattern: stringOrEmpty(input.filenamePattern, 240),
  };
}

export function loadExportSettings(storage = browserStorage()): ExportSettings {
  const raw = safeGetItem(EXPORT_SETTINGS_STORAGE_KEY, storage);
  if (!raw) return { ...DEFAULT_EXPORT_SETTINGS };
  try {
    return normalizeExportSettings(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_EXPORT_SETTINGS };
  }
}

export function saveExportSettings(
  settings: ExportSettings,
  storage = browserStorage(),
): ExportSettings {
  const normalized = normalizeExportSettings(settings);
  safeSetItem(EXPORT_SETTINGS_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [exportSettingsStore, setExportSettingsStore] = createStore<ExportSettings>(
  loadExportSettings(),
);

export const exportSettings = exportSettingsStore;

export function updateExportSettings(
  patch: Partial<ExportSettings>,
  storage = browserStorage(),
): ExportSettings {
  const next = normalizeExportSettings({ ...exportSettingsStore, ...patch });
  setExportSettingsStore(next);
  saveExportSettings(next, storage);
  return next;
}