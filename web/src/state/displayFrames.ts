import type { DataPayload } from "~/adapters/wss";
import { formatTimestampNs, type DisplaySettings } from "~/state/displaySettings";

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

const utf8Decoder = new TextDecoder("utf-8", { fatal: false });

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

export function logTypeLabel(payload: Pick<DataPayload, "kind" | "tags">): string {
  const tags = payload.tags && payload.tags.length > 0 ? `:${payload.tags.join("|")}` : "";
  return `${payload.kind}${tags}`;
}

export function metadataPrefix(
  payload: DataPayload,
  settings: Pick<DisplaySettings, "showTimestamp" | "showKind" | "showSource" | "timezone">,
  sourceLabel: string,
): string {
  const parts: string[] = [];
  if (settings.showTimestamp) {
    parts.push(formatTimestampNs(payload.ts_origin, settings));
  }
  if (settings.showKind) {
    parts.push(logTypeLabel(payload));
  }
  if (settings.showSource) {
    parts.push(sourceLabel);
  }
  return parts.length > 0 ? `[${parts.join(" ")}] ` : "";
}

export function bodyText(payload: Pick<DataPayload, "body">): string {
  if (payload.body instanceof Uint8Array) {
    return utf8Decoder.decode(payload.body);
  }
  if (typeof payload.body === "object" && payload.body) {
    return JSON.stringify(payload.body);
  }
  return "";
}

export function renderPayload(
  payload: DataPayload,
  settings: Pick<DisplaySettings, "showTimestamp" | "showKind" | "showSource" | "timezone">,
  sourceLabel: string,
): RenderedPayload {
  const prefix = metadataPrefix(payload, settings, sourceLabel);
  return {
    text: `${prefix}${bodyText(payload)}`,
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
): boolean {
  if (filter.kind !== "all" && payload.kind !== filter.kind) return false;

  const tagTerms = queryTerms(filter.tagQuery);
  if (tagTerms.length > 0) {
    const haystack = [payload.kind, ...(payload.tags ?? [])]
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
