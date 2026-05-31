import { resolveTraceMuxHttpUrl, resolveTraceMuxToken } from "~/adapters/wss";

export type SessionExportFormat = "text" | "csv" | "jsonl" | "pcapng";

export interface SessionExportOptions {
  format: SessionExportFormat;
  timezone?: string;
  encoding?: string;
  filenamePattern?: string;
  sourceName?: string;
  timestamp?: Date | number | string;
}

export interface ExportTicketResponse {
  ticket: string;
  expires_in_ms: number;
  expires_at_ms: number;
}

export const DEFAULT_SESSION_EXPORT_FILENAME_PATTERN = "tracemux-{sid}.{ext}";

export function sessionExportUrl(sid: string, options: SessionExportOptions): string {
  const params = new URLSearchParams({ format: options.format });
  const timezone = options.timezone?.trim();
  if (timezone) params.set("tz", timezone);
  const encoding = options.encoding?.trim();
  if (encoding) params.set("encoding", encoding);
  const filename = renderSessionExportFilename(options.filenamePattern, {
    sid,
    format: options.format,
    sourceName: options.sourceName,
    timestamp: options.timestamp,
  });
  params.set("filename", filename);
  const base = resolveTraceMuxHttpUrl(`/api/sessions/${encodeURIComponent(sid)}/export`);
  return `${base}?${params}`;
}

export function sessionExportTicketUrl(sid: string, options: SessionExportOptions): string {
  const params = new URLSearchParams({ format: options.format });
  const timezone = options.timezone?.trim();
  if (timezone) params.set("tz", timezone);
  const encoding = options.encoding?.trim();
  if (encoding) params.set("encoding", encoding);
  const base = resolveTraceMuxHttpUrl(`/api/sessions/${encodeURIComponent(sid)}/export-ticket`);
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
  return sanitized || "tracemux-export";
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

export async function fetchSessionExportBlob(
  sid: string,
  options: SessionExportOptions,
): Promise<Blob> {
  const headers: HeadersInit = {};
  const token = resolveTraceMuxToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  const response = await fetch(sessionExportUrl(sid, options), { headers });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(detail || `export failed: HTTP ${response.status}`);
  }
  return response.blob();
}

export async function requestSessionExportTicket(
  sid: string,
  options: SessionExportOptions,
): Promise<ExportTicketResponse> {
  const headers: HeadersInit = {};
  const token = resolveTraceMuxToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  const response = await fetch(sessionExportTicketUrl(sid, options), {
    method: "POST",
    headers,
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(detail || `export ticket failed: HTTP ${response.status}`);
  }
  return response.json() as Promise<ExportTicketResponse>;
}

export async function sessionExportDownloadUrl(
  sid: string,
  options: SessionExportOptions,
): Promise<string> {
  const url = new URL(sessionExportUrl(sid, options));
  if (resolveTraceMuxToken()) {
    const { ticket } = await requestSessionExportTicket(sid, options);
    url.searchParams.set("ticket", ticket);
  }
  return url.toString();
}

export function downloadBlob(blob: Blob, filename: string): void {
  const href = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = href;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(href);
  }
}

export function downloadUrl(url: string, filename: string): void {
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
}

export async function downloadSessionExport(
  sid: string,
  options: SessionExportOptions,
): Promise<void> {
  const filename = renderSessionExportFilename(options.filenamePattern, {
    sid,
    format: options.format,
    sourceName: options.sourceName,
    timestamp: options.timestamp,
  });
  downloadUrl(await sessionExportDownloadUrl(sid, options), filename);
}
