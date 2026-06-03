import { createStore } from "solid-js/store";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const DISPLAY_SETTINGS_STORAGE_KEY = "tracemux.displaySettings.v1";

export const DISPLAY_SETTING_LIMITS = {
  terminalScrollback: { min: 100, max: 1_000_000 },
  tileScrollback: { min: 50, max: 100_000 },
  terminalMaxRecords: { min: 100, max: 1_000_000 },
  tileMaxRecords: { min: 50, max: 100_000 },
  tileMinWidth: { min: 120, max: 1200 },
  tileMinHeight: { min: 80, max: 900 },
} as const;

export interface DisplaySettings {
  terminalScrollback: number;
  tileScrollback: number;
  terminalMaxRecords: number;
  tileMaxRecords: number;
  tileMinWidth: number;
  tileMinHeight: number;
  showTimestamp: boolean;
  showKind: boolean;
  showSource: boolean;
  timezone: string;
  tileRenderingPaused: boolean;
}

export const DEFAULT_DISPLAY_SETTINGS: DisplaySettings = {
  terminalScrollback: 10_000,
  tileScrollback: 500,
  terminalMaxRecords: 5_000,
  tileMaxRecords: 500,
  tileMinWidth: 240,
  tileMinHeight: 160,
  showTimestamp: false,
  showKind: false,
  showSource: false,
  timezone: "local",
  tileRenderingPaused: false,
};

function clampInt(value: unknown, fallback: number, min: number, max: number): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(parsed)));
}

function boolOr(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function stringOr(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : fallback;
}

export function normalizeDisplaySettings(value: unknown): DisplaySettings {
  const input = value && typeof value === "object" ? value as Partial<DisplaySettings> : {};
  return {
    terminalScrollback: clampInt(
      input.terminalScrollback,
      DEFAULT_DISPLAY_SETTINGS.terminalScrollback,
      DISPLAY_SETTING_LIMITS.terminalScrollback.min,
      DISPLAY_SETTING_LIMITS.terminalScrollback.max,
    ),
    tileScrollback: clampInt(
      input.tileScrollback,
      DEFAULT_DISPLAY_SETTINGS.tileScrollback,
      DISPLAY_SETTING_LIMITS.tileScrollback.min,
      DISPLAY_SETTING_LIMITS.tileScrollback.max,
    ),
    terminalMaxRecords: clampInt(
      input.terminalMaxRecords,
      DEFAULT_DISPLAY_SETTINGS.terminalMaxRecords,
      DISPLAY_SETTING_LIMITS.terminalMaxRecords.min,
      DISPLAY_SETTING_LIMITS.terminalMaxRecords.max,
    ),
    tileMaxRecords: clampInt(
      input.tileMaxRecords,
      DEFAULT_DISPLAY_SETTINGS.tileMaxRecords,
      DISPLAY_SETTING_LIMITS.tileMaxRecords.min,
      DISPLAY_SETTING_LIMITS.tileMaxRecords.max,
    ),
    tileMinWidth: clampInt(
      input.tileMinWidth,
      DEFAULT_DISPLAY_SETTINGS.tileMinWidth,
      DISPLAY_SETTING_LIMITS.tileMinWidth.min,
      DISPLAY_SETTING_LIMITS.tileMinWidth.max,
    ),
    tileMinHeight: clampInt(
      input.tileMinHeight,
      DEFAULT_DISPLAY_SETTINGS.tileMinHeight,
      DISPLAY_SETTING_LIMITS.tileMinHeight.min,
      DISPLAY_SETTING_LIMITS.tileMinHeight.max,
    ),
    showTimestamp: boolOr(input.showTimestamp, DEFAULT_DISPLAY_SETTINGS.showTimestamp),
    showKind: boolOr(input.showKind, DEFAULT_DISPLAY_SETTINGS.showKind),
    showSource: boolOr(input.showSource, DEFAULT_DISPLAY_SETTINGS.showSource),
    timezone: stringOr(input.timezone, DEFAULT_DISPLAY_SETTINGS.timezone),
    tileRenderingPaused: boolOr(
      input.tileRenderingPaused,
      DEFAULT_DISPLAY_SETTINGS.tileRenderingPaused,
    ),
  };
}

export function loadDisplaySettings(storage = browserStorage()): DisplaySettings {
  const raw = safeGetItem(DISPLAY_SETTINGS_STORAGE_KEY, storage);
  if (!raw) return { ...DEFAULT_DISPLAY_SETTINGS };
  try {
    return normalizeDisplaySettings(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_DISPLAY_SETTINGS };
  }
}

export function saveDisplaySettings(
  settings: DisplaySettings,
  storage = browserStorage(),
): DisplaySettings {
  const normalized = normalizeDisplaySettings(settings);
  safeSetItem(DISPLAY_SETTINGS_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [displaySettingsStore, setDisplaySettingsStore] = createStore<DisplaySettings>(
  loadDisplaySettings(),
);

export const displaySettings = displaySettingsStore;

export function updateDisplaySettings(
  patch: Partial<DisplaySettings>,
  storage = browserStorage(),
): DisplaySettings {
  const next = normalizeDisplaySettings({ ...displaySettingsStore, ...patch });
  setDisplaySettingsStore(next);
  saveDisplaySettings(next, storage);
  return next;
}

export function resetDisplaySettings(storage = browserStorage()): DisplaySettings {
  const next = { ...DEFAULT_DISPLAY_SETTINGS };
  setDisplaySettingsStore(next);
  saveDisplaySettings(next, storage);
  return next;
}

function nsToDate(tsNs: bigint | number): Date {
  if (typeof tsNs === "bigint") {
    return new Date(Number(tsNs / 1_000_000n));
  }
  return new Date(Number(tsNs) / 1_000_000);
}

function parseGmtOffsetMinutes(timezone: string): number | null {
  const match = /^(?:GMT)?([+-])(\d{1,2})(?::?(\d{2}))?$/i.exec(timezone.trim());
  if (!match) return null;
  const signToken = match[1];
  const hourToken = match[2];
  if (!signToken || !hourToken) return null;
  const hours = Number(hourToken);
  const minutes = Number(match[3] ?? "0");
  if (!Number.isInteger(hours) || !Number.isInteger(minutes)) return null;
  if (hours > 14 || minutes > 59) return null;
  const sign = signToken === "+" ? 1 : -1;
  return sign * (hours * 60 + minutes);
}

export function isValidDisplayTimezone(value: string): boolean {
  const timezone = value.trim();
  if (!timezone || timezone === "local") return true;
  if (parseGmtOffsetMinutes(timezone) !== null) return true;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: timezone }).format(0);
    return true;
  } catch {
    return false;
  }
}

function formatDateTime(date: Date, timezone: string | undefined): string {
  return new Intl.DateTimeFormat("sv-SE", {
    ...(timezone ? { timeZone: timezone } : {}),
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    fractionalSecondDigits: 3,
    hour12: false,
  }).format(date);
}

export function formatTimestampNs(
  tsNs: bigint | number,
  settings: Pick<DisplaySettings, "timezone"> = displaySettingsStore,
): string {
  const timezone = settings.timezone.trim();
  const date = nsToDate(tsNs);
  if (timezone === "local") return formatDateTime(date, undefined);

  const offsetMinutes = parseGmtOffsetMinutes(timezone);
  if (offsetMinutes !== null) {
    const shifted = new Date(date.getTime() + offsetMinutes * 60_000);
    return `${formatDateTime(shifted, "UTC")} ${timezone.toUpperCase()}`;
  }

  try {
    return `${formatDateTime(date, timezone)} ${timezone}`;
  } catch {
    return formatDateTime(date, undefined);
  }
}
