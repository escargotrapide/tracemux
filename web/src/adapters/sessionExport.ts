import { resolveWanloggerHttpUrl, resolveWanloggerToken } from "~/adapters/wss";

export type SessionExportFormat = "text" | "csv" | "jsonl";

export interface SessionExportOptions {
  format: SessionExportFormat;
  timezone?: string;
}

export function sessionExportUrl(sid: string, options: SessionExportOptions): string {
  const params = new URLSearchParams({ format: options.format });
  const timezone = options.timezone?.trim();
  if (timezone) params.set("tz", timezone);
  const base = resolveWanloggerHttpUrl(`/api/sessions/${encodeURIComponent(sid)}/export`);
  return `${base}?${params}`;
}

export function sessionExportFilename(sid: string, format: SessionExportFormat): string {
  const extension = format === "text" ? "txt" : format;
  return `wanlogger-${sid}.${extension}`;
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
    a.download = sessionExportFilename(sid, options.format);
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(href);
  }
}
