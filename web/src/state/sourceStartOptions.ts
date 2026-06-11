import { createStore } from "solid-js/store";
import { wireClassificationRules } from "~/state/classificationRules";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const SOURCE_START_OPTIONS_STORAGE_KEY = "tracemux.sourceStartOptions.v1";
export const DEFAULT_SOURCE_ENCODING = "utf-8";
export const SUPPORTED_SOURCE_ENCODINGS = [
  "utf-8",
  "shift_jis",
  "cp932",
  "euc-jp",
  "iso-2022-jp",
] as const;
export const SUPPORTED_DETECTION_MODES = [
  "configured",
  "auto",
  "suggest",
  "monitor",
  "off",
] as const;

export type DetectionMode = typeof SUPPORTED_DETECTION_MODES[number];

/// Default observation window (seconds) for the `monitor` detection mode.
export const DEFAULT_MONITOR_WINDOW_SECONDS = 30;
/// Safe range for the `monitor` window in seconds.
export const MONITOR_WINDOW_SECONDS_LIMITS = { min: 1, max: 3600 } as const;

export interface SourceStartOptions {
  encoding: string;
  detectionMode: DetectionMode;
  monitorWindowSeconds: number;
  sessionNamePattern: string;
  sendClassificationRules: boolean;
}

export interface StartCtlOptions {
  encoding?: string;
  detection_mode?: DetectionMode;
  monitor_window_secs?: number;
  session_name_pattern?: string;
  classifier?: ReturnType<typeof wireClassificationRules>;
}

export const DEFAULT_SOURCE_START_OPTIONS: SourceStartOptions = {
  encoding: DEFAULT_SOURCE_ENCODING,
  detectionMode: "configured",
  monitorWindowSeconds: DEFAULT_MONITOR_WINDOW_SECONDS,
  sessionNamePattern: "",
  sendClassificationRules: true,
};

export function normalizeEncoding(value: unknown): string {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  return raw || DEFAULT_SOURCE_ENCODING;
}

export function normalizeDetectionMode(value: unknown): DetectionMode {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  return SUPPORTED_DETECTION_MODES.includes(raw as DetectionMode)
    ? raw as DetectionMode
    : "configured";
}

export function normalizeMonitorWindowSeconds(value: unknown): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return DEFAULT_MONITOR_WINDOW_SECONDS;
  const truncated = Math.trunc(n);
  return Math.min(
    Math.max(truncated, MONITOR_WINDOW_SECONDS_LIMITS.min),
    MONITOR_WINDOW_SECONDS_LIMITS.max,
  );
}

export function normalizeSourceStartOptions(value: unknown): SourceStartOptions {
  if (!value || typeof value !== "object") return { ...DEFAULT_SOURCE_START_OPTIONS };
  const input = value as Partial<SourceStartOptions>;
  return {
    encoding: normalizeEncoding(input.encoding),
    detectionMode: normalizeDetectionMode(input.detectionMode),
    monitorWindowSeconds: normalizeMonitorWindowSeconds(input.monitorWindowSeconds),
    sessionNamePattern: typeof input.sessionNamePattern === "string"
      ? input.sessionNamePattern.trim().slice(0, 240)
      : "",
    sendClassificationRules: input.sendClassificationRules !== false,
  };
}

export function loadSourceStartOptions(storage = browserStorage()): SourceStartOptions {
  const raw = safeGetItem(SOURCE_START_OPTIONS_STORAGE_KEY, storage);
  if (!raw) return { ...DEFAULT_SOURCE_START_OPTIONS };
  try {
    return normalizeSourceStartOptions(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_SOURCE_START_OPTIONS };
  }
}

export function saveSourceStartOptions(
  options: SourceStartOptions,
  storage = browserStorage(),
): SourceStartOptions {
  const normalized = normalizeSourceStartOptions(options);
  safeSetItem(SOURCE_START_OPTIONS_STORAGE_KEY, JSON.stringify(normalized), storage);
  return normalized;
}

const [sourceStartOptionsStore, setSourceStartOptionsStore] = createStore<SourceStartOptions>(
  loadSourceStartOptions(),
);

export const sourceStartOptions = sourceStartOptionsStore;

export function updateSourceStartOptions(
  patch: Partial<SourceStartOptions>,
  storage = browserStorage(),
): SourceStartOptions {
  const next = normalizeSourceStartOptions({ ...sourceStartOptionsStore, ...patch });
  setSourceStartOptionsStore(next);
  saveSourceStartOptions(next, storage);
  return next;
}

export function resetSourceStartOptions(storage = browserStorage()): SourceStartOptions {
  const next = { ...DEFAULT_SOURCE_START_OPTIONS };
  setSourceStartOptionsStore(next);
  saveSourceStartOptions(next, storage);
  return next;
}

export function startCtlOptions(
  options: SourceStartOptions = sourceStartOptionsStore,
): StartCtlOptions {
  const normalized = normalizeSourceStartOptions(options);
  const out: StartCtlOptions = {
    encoding: normalized.encoding,
    detection_mode: normalized.detectionMode,
  };
  if (normalized.detectionMode === "monitor") {
    out.monitor_window_secs = normalized.monitorWindowSeconds;
  }
  if (normalized.sessionNamePattern) {
    out.session_name_pattern = normalized.sessionNamePattern;
  }
  if (normalized.detectionMode !== "off" && normalized.sendClassificationRules) {
    const rules = wireClassificationRules();
    if (rules.length > 0) out.classifier = rules;
  }
  return out;
}
