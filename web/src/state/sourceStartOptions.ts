import { createStore } from "solid-js/store";
import { wireClassificationRules } from "~/state/classificationRules";

export const SOURCE_START_OPTIONS_STORAGE_KEY = "wanlogger.sourceStartOptions.v1";
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
  "off",
] as const;

export type DetectionMode = typeof SUPPORTED_DETECTION_MODES[number];

export interface SourceStartOptions {
  encoding: string;
  detectionMode: DetectionMode;
  sessionNamePattern: string;
  sendClassificationRules: boolean;
}

export interface StartCtlOptions {
  encoding?: string;
  detection_mode?: DetectionMode;
  session_name_pattern?: string;
  classifier?: ReturnType<typeof wireClassificationRules>;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

export const DEFAULT_SOURCE_START_OPTIONS: SourceStartOptions = {
  encoding: DEFAULT_SOURCE_ENCODING,
  detectionMode: "configured",
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

export function normalizeSourceStartOptions(value: unknown): SourceStartOptions {
  if (!value || typeof value !== "object") return { ...DEFAULT_SOURCE_START_OPTIONS };
  const input = value as Partial<SourceStartOptions>;
  return {
    encoding: normalizeEncoding(input.encoding),
    detectionMode: normalizeDetectionMode(input.detectionMode),
    sessionNamePattern: typeof input.sessionNamePattern === "string"
      ? input.sessionNamePattern.trim().slice(0, 240)
      : "",
    sendClassificationRules: input.sendClassificationRules !== false,
  };
}

export function loadSourceStartOptions(storage = defaultStorage()): SourceStartOptions {
  const raw = storage?.getItem(SOURCE_START_OPTIONS_STORAGE_KEY) ?? null;
  if (!raw) return { ...DEFAULT_SOURCE_START_OPTIONS };
  try {
    return normalizeSourceStartOptions(JSON.parse(raw) as unknown);
  } catch {
    return { ...DEFAULT_SOURCE_START_OPTIONS };
  }
}

export function saveSourceStartOptions(
  options: SourceStartOptions,
  storage = defaultStorage(),
): SourceStartOptions {
  const normalized = normalizeSourceStartOptions(options);
  storage?.setItem(SOURCE_START_OPTIONS_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [sourceStartOptionsStore, setSourceStartOptionsStore] = createStore<SourceStartOptions>(
  loadSourceStartOptions(),
);

export const sourceStartOptions = sourceStartOptionsStore;

export function updateSourceStartOptions(
  patch: Partial<SourceStartOptions>,
  storage = defaultStorage(),
): SourceStartOptions {
  const next = normalizeSourceStartOptions({ ...sourceStartOptionsStore, ...patch });
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
  if (normalized.sessionNamePattern) {
    out.session_name_pattern = normalized.sessionNamePattern;
  }
  if (normalized.detectionMode !== "off" && normalized.sendClassificationRules) {
    const rules = wireClassificationRules();
    if (rules.length > 0) out.classifier = rules;
  }
  return out;
}
