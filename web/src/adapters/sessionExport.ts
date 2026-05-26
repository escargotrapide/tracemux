import { resolveWanloggerHttpUrl, resolveWanloggerToken } from "~/adapters/wss";

export type SessionExportFormat = "text" | "csv" | "jsonl" | "pcapng";

export interface SessionExportOptions {
  format: SessionExportFormat;
  timezone?: string;
  encoding?: string;
  filenamePattern?: string;
  sourceName?: string;
  timestamp?: Date | number | string;
}

export const DEFAULT_SESSION_EXPORT_FILENAME_PATTERN = "wanlogger-{sid}.{ext}";

export function sessionExportUrl(sid: string, options: SessionExportOptions): string {
  const params = new URLSearchParams({ format: options.format });
  const timezone = options.timezone?.trim();
  if (timezone) params.set("tz", timezone);
  const encoding = options.encoding?.trim();
  if (encoding) params.set("encoding", encoding);
  const base = resolveWanloggerHttpUrl(`/api/sessions/${encodeURIComponent(sid)}/export`);
  return `${base}?${params}`;
}

export function sessionExportExtension(format: SessionExportFormat): string {
  return format === "text" ? "txt" : format;
}

function timestampToken(value: Date | number | string | undefined): string {
  const date = value instanceof Date
    ? value
    : typeof value === "number"
      ? new Date(value)
      : typeof value === "string" && value.trim()
        ? new Date(value)
        : new Date();
  if (!Number.isFinite(date.getTime())) return "unknown-time";
  return date.toISOString().replace(/\.\d{3}Z$/, "Z").replace(/:/g, "");
}

export function sanitizeExportFilename(value: string): string {
  const sanitized = value
    .replace(/[<>:"/\\|?*\u0000-\u001F]+/g, "-")
    .replace(/\s+/g, " ")
    .replace(/\.+$/g, "")
    .trim();
  return sanitized || "wanlogger-export";
}

interface SessionExportFilenameContext {
  sid: string;
  format: SessionExportFormat;
  sourceName?: string | undefined;
  timestamp?: Date | number | string | undefined;
}

export function renderSessionExportFilename(
  pattern: string | undefined,
  context: SessionExportFilenameContext,
): string {
  const ext = sessionExportExtension(context.format);
  const source = context.sourceName?.trim() || context.sid;
  const template = pattern?.trim() || DEFAULT_SESSION_EXPORT_FILENAME_PATTERN;
  const rendered = template
    .replaceAll("{sid}", context.sid)
    .replaceAll("{source}", source)
    .replaceAll("{timestamp}", timestampToken(context.timestamp))
    .replaceAll("{format}", context.format)
    .replaceAll("{ext}", ext);
  const filename = sanitizeExportFilename(rendered);
  return filename.toLowerCase().endsWith(`.${ext}`) ? filename : `${filename}.${ext}`;
}

export function sessionExportFilename(sid: string, format: SessionExportFormat): string {
  return renderSessionExportFilename(DEFAULT_SESSION_EXPORT_FILENAME_PATTERN, { sid, format });
}

export async function downloadSessionExport(
  sid: string,
  options: SessionExportOptions,
): Promise<void> {
  const headers: HeadersInit = {};
  const token = resolveWanloggerToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  const response = await fetch(sessionExportUrl(sid, options), { headers });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(detail || `export failed: HTTP ${response.status}`);
  }
  const blob = await response.blob();
  const href = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = href;
    a.download = renderSessionExportFilename(options.filenamePattern, {
      sid,
      format: options.format,
      sourceName: options.sourceName,
      timestamp: options.timestamp,
    });
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(href);
  }
}
