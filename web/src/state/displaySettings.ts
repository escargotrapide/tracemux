import { createStore } from "solid-js/store";

export const DISPLAY_SETTINGS_STORAGE_KEY = "wanlogger.displaySettings.v1";

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
};

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

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
      100,
      1_000_000,
    ),
    tileScrollback: clampInt(
      input.tileScrollback,
      DEFAULT_DISPLAY_SETTINGS.tileScrollback,
      50,
      100_000,
    ),
    terminalMaxRecords: clampInt(
      input.terminalMaxRecords,
      DEFAULT_DISPLAY_SETTINGS.terminalMaxRecords,
      100,
      1_000_000,
    ),
    tileMaxRecords: clampInt(
      input.tileMaxRecords,
      DEFAULT_DISPLAY_SETTINGS.tileMaxRecords,
      50,
      100_000,
    ),
    tileMinWidth: clampInt(input.tileMinWidth, DEFAULT_DISPLAY_SETTINGS.tileMinWidth, 120, 1200),
    tileMinHeight: clampInt(input.tileMinHeight, DEFAULT_DISPLAY_SETTINGS.tileMinHeight, 80, 900),
    showTimestamp: boolOr(input.showTimestamp, DEFAULT_DISPLAY_SETTINGS.showTimestamp),
    showKind: boolOr(input.showKind, DEFAULT_DISPLAY_SETTINGS.showKind),
    showSource: boolOr(input.showSource, DEFAULT_DISPLAY_SETTINGS.showSource),
    timezone: stringOr(input.timezone, DEFAULT_DISPLAY_SETTINGS.timezone),
  };
}

export function loadDisplaySettings(storage = defaultStorage()): DisplaySettings {
  const raw = storage?.getItem(DISPLAY_SETTINGS_STORAGE_KEY) ?? null;
  if (!raw) return { ...DEFAULT_DISPLAY_SETTINGS };
  try {
    return normalizeDisplaySettings(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_DISPLAY_SETTINGS };
  }
}

export function saveDisplaySettings(
  settings: DisplaySettings,
  storage = defaultStorage(),
): DisplaySettings {
  const normalized = normalizeDisplaySettings(settings);
  storage?.setItem(DISPLAY_SETTINGS_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [displaySettingsStore, setDisplaySettingsStore] = createStore<DisplaySettings>(
  loadDisplaySettings(),
);

export const displaySettings = displaySettingsStore;

export function updateDisplaySettings(
  patch: Partial<DisplaySettings>,
  storage = defaultStorage(),
): DisplaySettings {
  const next = normalizeDisplaySettings({ ...displaySettingsStore, ...patch });
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
  const match = /^GMT([+-])(\d{1,2})(?::?(\d{2}))?$/i.exec(timezone.trim());
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
