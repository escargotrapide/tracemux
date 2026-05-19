import type { DataPayload } from "~/adapters/wss";
import { classifyText, type ClassificationRule } from "~/state/classificationRules";
import { formatTimestampNs, type DisplaySettings } from "~/state/displaySettings";
import { DEFAULT_SOURCE_ENCODING } from "~/state/sourceStartOptions";

export type DataKind = DataPayload["kind"];

export interface DisplaySourceInfo {
  name: string;
}

export type DisplaySourceLookup = Record<string, DisplaySourceInfo | undefined>;

export interface DisplayAliasInfo {
  label: string;
}

export type DisplayAliasLookup = Record<string, DisplayAliasInfo | undefined>;

export interface DisplayPayloadSource {
  sid: string;
  source?: string;
}

export interface DisplayFilter {
  kind: DataKind | "all";
  tagQuery: string;
  sourceQuery: string;
}

export const DEFAULT_DISPLAY_FILTER: DisplayFilter = {
  kind: "all",
  tagQuery: "",
  sourceQuery: "",
};

export interface RenderedPayload {
  text: string;
  newline: boolean;
}

const decoders = new Map<string, TextDecoder>();

function trimmed(value: string | undefined): string | null {
  const next = value?.trim();
  return next ? next : null;
}

export function labelForSid(
  sid: string,
  sources: DisplaySourceLookup,
  aliases: DisplayAliasLookup = {},
): string {
  return trimmed(aliases[sid]?.label)
    ?? trimmed(sources[sid]?.name)
    ?? sid.slice(0, 8);
}

export function sourceDisplayName(
  payload: DisplayPayloadSource,
  sources: DisplaySourceLookup,
  aliases: DisplayAliasLookup = {},
): string {
  return trimmed(aliases[payload.sid]?.label)
    ?? trimmed(payload.source)
    ?? trimmed(sources[payload.sid]?.name)
    ?? payload.sid.slice(0, 8);
}

export function mergedTags(
  payload: Pick<DataPayload, "tags">,
  extraTags: string[] = [],
): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const tag of [...(payload.tags ?? []), ...extraTags]) {
    const normalized = tag.trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

export function logTypeLabel(
  payload: Pick<DataPayload, "kind" | "tags">,
  extraTags: string[] = [],
): string {
  const tags = mergedTags(payload, extraTags);
  const suffix = tags.length > 0 ? `:${tags.join("|")}` : "";
  return `${payload.kind}${suffix}`;
}

export function metadataPrefix(
  payload: DataPayload,
  settings: Pick<DisplaySettings, "showTimestamp" | "showKind" | "showSource" | "timezone">,
  sourceLabel: string,
  extraTags: string[] = [],
): string {
  const parts: string[] = [];
  if (settings.showTimestamp) {
    parts.push(formatTimestampNs(payload.ts_origin, settings));
  }
  if (settings.showKind) {
    parts.push(logTypeLabel(payload, extraTags));
  }
  if (settings.showSource) {
    parts.push(sourceLabel);
  }
  return parts.length > 0 ? `[${parts.join(" ")}] ` : "";
}

function decoderForEncoding(encoding: string): TextDecoder {
  const label = (encoding || DEFAULT_SOURCE_ENCODING).trim().toLowerCase();
  const existing = decoders.get(label);
  if (existing) return existing;
  let decoder: TextDecoder;
  try {
    decoder = new TextDecoder(label, { fatal: false });
  } catch {
    decoder = new TextDecoder(DEFAULT_SOURCE_ENCODING, { fatal: false });
  }
  decoders.set(label, decoder);
  return decoder;
}

export function bodyText(
  payload: Pick<DataPayload, "body">,
  encoding = DEFAULT_SOURCE_ENCODING,
): string {
  if (payload.body instanceof Uint8Array) {
    return decoderForEncoding(encoding).decode(payload.body);
  }
  if (typeof payload.body === "object" && payload.body) {
    return JSON.stringify(payload.body);
  }
  return "";
}

export function clientClassificationTags(
  payload: Pick<DataPayload, "body">,
  rules: ClassificationRule[],
  encoding = DEFAULT_SOURCE_ENCODING,
): string[] {
  if (rules.length === 0) return [];
  return classifyText(bodyText(payload, encoding), rules);
}

export function renderPayload(
  payload: DataPayload,
  settings: Pick<DisplaySettings, "showTimestamp" | "showKind" | "showSource" | "timezone">,
  sourceLabel: string,
  options: { encoding?: string; extraTags?: string[] } = {},
): RenderedPayload {
  const prefix = metadataPrefix(payload, settings, sourceLabel, options.extraTags ?? []);
  return {
    text: `${prefix}${bodyText(payload, options.encoding)}`,
    newline: !(payload.body instanceof Uint8Array),
  };
}

function queryTerms(query: string): string[] {
  return query
    .split(",")
    .map((item) => item.trim().toLowerCase())
    .filter(Boolean);
}

export function payloadMatchesFilter(
  payload: DataPayload,
  filter: DisplayFilter,
  sourceLabel: string,
  extraTags: string[] = [],
): boolean {
  if (filter.kind !== "all" && payload.kind !== filter.kind) return false;

  const tagTerms = queryTerms(filter.tagQuery);
  if (tagTerms.length > 0) {
    const haystack = [payload.kind, ...mergedTags(payload, extraTags)]
      .map((item) => item.toLowerCase());
    if (!tagTerms.some((term) => haystack.some((item) => item.includes(term)))) {
      return false;
    }
  }

  const sourceTerms = queryTerms(filter.sourceQuery);
  if (sourceTerms.length > 0) {
    const haystack = `${sourceLabel} ${payload.source ?? ""} ${payload.sid}`.toLowerCase();
    if (!sourceTerms.some((term) => haystack.includes(term))) return false;
  }

  return true;
}
