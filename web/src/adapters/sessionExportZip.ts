// Client-side ZIP packaging remains available for small tests/fallbacks, but
// production all-sources downloads use the server-side bundle API so large
// exports do not have to be materialized in browser memory.
// REQ: FR-UI-018

import {
  downloadUrl,
  fetchSessionExportBlob,
  renderSessionExportFilename,
  sanitizeExportFilename,
  type SessionExportFormat,
  type SessionExportOptions,
} from "~/adapters/sessionExport";
import { resolveWanloggerHttpUrl, resolveWanloggerToken } from "~/adapters/wss";

export interface SessionExportZipEntry {
  sid: string;
  sourceName?: string | undefined;
  encoding?: string | undefined;
}

export interface SessionExportZipProgress {
  completed: number;
  total: number;
  sid: string;
  sourceName?: string | undefined;
}

export interface SessionExportZipOptions {
  format: SessionExportFormat;
  timezone?: string | undefined;
  filenamePattern?: string | undefined;
  timestamp?: Date | number | string | undefined;
  fetchExportBlob?: ((sid: string, options: SessionExportOptions) => Promise<Blob>) | undefined;
  onProgress?: ((progress: SessionExportZipProgress) => void) | undefined;
}

export interface SessionExportZipResult {
  blob?: Blob;
  filename: string;
  entryNames: string[];
  downloadUrl?: string;
}

export interface ZipFile {
  name: string;
  body: Uint8Array;
}

const ZIP_MIME = "application/zip";
const textEncoder = new TextEncoder();
let crcTable: Uint32Array | undefined;

interface ServerBundleTicketResponse {
  ticket: string;
  expires_in_ms: number;
  expires_at_ms: number;
}

interface ServerBundleTicketRequest {
  entries: Array<{
    sid: string;
    source_name?: string;
    encoding?: string;
  }>;
  format: SessionExportFormat;
  tz?: string;
  filename_pattern?: string;
  timestamp_ms: number;
}

function timestampDate(value: Date | number | string | undefined): Date {
  const date = value instanceof Date
    ? value
    : typeof value === "number"
      ? new Date(value)
      : typeof value === "string" && value.trim()
        ? new Date(value)
        : new Date();
  return Number.isFinite(date.getTime()) ? date : new Date(0);
}

function timestampToken(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z").replace(/:/g, "");
}

export function sessionExportZipBaseName(
  format: SessionExportFormat,
  timestamp: Date | number | string | undefined = new Date(),
): string {
  return sanitizeExportFilename(`wanlogger-all-${timestampToken(timestampDate(timestamp))}-${format}`);
}

export function sessionExportZipFilename(
  format: SessionExportFormat,
  timestamp: Date | number | string | undefined = new Date(),
): string {
  return `${sessionExportZipBaseName(format, timestamp)}.zip`;
}

function sessionExportBundleTicketUrl(): string {
  return resolveWanloggerHttpUrl("/api/exports/bundle-ticket");
}

function sessionExportBundleDownloadUrl(ticket: string): string {
  const url = new URL(resolveWanloggerHttpUrl("/api/exports/bundle"));
  url.searchParams.set("ticket", ticket);
  return url.toString();
}

async function requestSessionExportBundleTicket(
  entries: SessionExportZipEntry[],
  options: SessionExportZipOptions,
  timestamp: Date,
): Promise<ServerBundleTicketResponse> {
  const headers: HeadersInit = { "Content-Type": "application/json" };
  const token = resolveWanloggerToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  const timezone = options.timezone?.trim();
  const filenamePattern = options.filenamePattern?.trim();
  const body: ServerBundleTicketRequest = {
    entries: entries.map((entry) => ({
      sid: entry.sid,
      ...(entry.sourceName !== undefined ? { source_name: entry.sourceName } : {}),
      ...(entry.encoding !== undefined ? { encoding: entry.encoding } : {}),
    })),
    format: options.format,
    timestamp_ms: timestamp.getTime(),
    ...(timezone ? { tz: timezone } : {}),
    ...(filenamePattern ? { filename_pattern: filenamePattern } : {}),
  };
  const response = await fetch(sessionExportBundleTicketUrl(), {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(detail || `bundle export ticket failed: HTTP ${response.status}`);
  }
  return response.json() as Promise<ServerBundleTicketResponse>;
}

function serverBundleEntryNames(
  entries: SessionExportZipEntry[],
  options: SessionExportZipOptions,
  timestamp: Date,
): string[] {
  const folder = sessionExportZipBaseName(options.format, timestamp);
  const used = new Set<string>();
  return entries.map((entry) => {
    const filename = renderSessionExportFilename(options.filenamePattern, {
      sid: entry.sid,
      format: options.format,
      sourceName: entry.sourceName,
      timestamp,
    });
    return `${folder}/${uniqueName(filename, used)}`;
  });
}

function uniqueName(name: string, used: Set<string>): string {
  const normalized = name.replace(/\\/g, "/");
  if (!used.has(normalized)) {
    used.add(normalized);
    return normalized;
  }

  const dot = normalized.lastIndexOf(".");
  const stem = dot > 0 ? normalized.slice(0, dot) : normalized;
  const ext = dot > 0 ? normalized.slice(dot) : "";
  for (let index = 2; index < 10_000; index += 1) {
    const candidate = `${stem}-${index}${ext}`;
    if (!used.has(candidate)) {
      used.add(candidate);
      return candidate;
    }
  }
  throw new Error("too many duplicate export filenames");
}

function crc32Table(): Uint32Array {
  if (crcTable) return crcTable;
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i += 1) {
    let value = i;
    for (let bit = 0; bit < 8; bit += 1) {
      value = (value & 1) !== 0 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    }
    table[i] = value >>> 0;
  }
  crcTable = table;
  return table;
}

function crc32(body: Uint8Array): number {
  const table = crc32Table();
  let crc = 0xffffffff;
  for (const byte of body) {
    crc = (table[(crc ^ byte) & 0xff] ?? 0) ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function dosDateTime(date: Date): { date: number; time: number } {
  const year = Math.max(1980, Math.min(2107, date.getFullYear()));
  return {
    date: ((year - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate(),
    time: (date.getHours() << 11) | (date.getMinutes() << 5) | Math.floor(date.getSeconds() / 2),
  };
}

function setU16(view: DataView, offset: number, value: number): void {
  view.setUint16(offset, value, true);
}

function setU32(view: DataView, offset: number, value: number): void {
  view.setUint32(offset, value >>> 0, true);
}

function appendChunk(chunks: Uint8Array[], chunk: Uint8Array): number {
  chunks.push(chunk);
  return chunk.byteLength;
}

function concatChunks(chunks: Uint8Array[], length: number): Uint8Array {
  const out = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}

export function createStoredZip(files: ZipFile[], modifiedAt = new Date()): Uint8Array {
  if (files.length > 0xffff) throw new Error("too many files for ZIP32 export");
  const chunks: Uint8Array[] = [];
  const centralChunks: Uint8Array[] = [];
  const { date, time } = dosDateTime(modifiedAt);
  let offset = 0;
  let centralSize = 0;

  for (const file of files) {
    const name = file.name.replace(/\\/g, "/");
    const nameBytes = textEncoder.encode(name);
    const body = file.body;
    if (nameBytes.byteLength > 0xffff) throw new Error(`ZIP entry name is too long: ${name}`);
    if (body.byteLength > 0xffffffff) throw new Error(`ZIP entry is too large: ${name}`);
    if (offset > 0xffffffff) throw new Error("ZIP export is too large for ZIP32");

    const checksum = crc32(body);
    const localOffset = offset;
    const local = new Uint8Array(30 + nameBytes.byteLength);
    const localView = new DataView(local.buffer);
    setU32(localView, 0, 0x04034b50);
    setU16(localView, 4, 20);
    setU16(localView, 6, 0x0800);
    setU16(localView, 8, 0);
    setU16(localView, 10, time);
    setU16(localView, 12, date);
    setU32(localView, 14, checksum);
    setU32(localView, 18, body.byteLength);
    setU32(localView, 22, body.byteLength);
    setU16(localView, 26, nameBytes.byteLength);
    setU16(localView, 28, 0);
    local.set(nameBytes, 30);
    offset += appendChunk(chunks, local);
    offset += appendChunk(chunks, body);

    const central = new Uint8Array(46 + nameBytes.byteLength);
    const centralView = new DataView(central.buffer);
    setU32(centralView, 0, 0x02014b50);
    setU16(centralView, 4, 20);
    setU16(centralView, 6, 20);
    setU16(centralView, 8, 0x0800);
    setU16(centralView, 10, 0);
    setU16(centralView, 12, time);
    setU16(centralView, 14, date);
    setU32(centralView, 16, checksum);
    setU32(centralView, 20, body.byteLength);
    setU32(centralView, 24, body.byteLength);
    setU16(centralView, 28, nameBytes.byteLength);
    setU16(centralView, 30, 0);
    setU16(centralView, 32, 0);
    setU16(centralView, 34, 0);
    setU16(centralView, 36, 0);
    setU32(centralView, 38, 0);
    setU32(centralView, 42, localOffset);
    central.set(nameBytes, 46);
    centralSize += appendChunk(centralChunks, central);
  }

  const centralOffset = offset;
  for (const central of centralChunks) {
    offset += appendChunk(chunks, central);
  }

  const eocd = new Uint8Array(22);
  const eocdView = new DataView(eocd.buffer);
  setU32(eocdView, 0, 0x06054b50);
  setU16(eocdView, 4, 0);
  setU16(eocdView, 6, 0);
  setU16(eocdView, 8, files.length);
  setU16(eocdView, 10, files.length);
  setU32(eocdView, 12, centralSize);
  setU32(eocdView, 16, centralOffset);
  setU16(eocdView, 20, 0);
  offset += appendChunk(chunks, eocd);

  return concatChunks(chunks, offset);
}

export async function createSessionExportZip(
  entries: SessionExportZipEntry[],
  options: SessionExportZipOptions,
): Promise<SessionExportZipResult> {
  if (entries.length === 0) throw new Error("no persisted sources to export");
  const timestamp = timestampDate(options.timestamp);
  const folder = sessionExportZipBaseName(options.format, timestamp);
  const fetchExportBlob = options.fetchExportBlob ?? fetchSessionExportBlob;
  const used = new Set<string>();
  const files: ZipFile[] = [];
  const entryNames: string[] = [];

  for (const [index, entry] of entries.entries()) {
    const exportOptions: SessionExportOptions = {
      format: options.format,
      timestamp,
      ...(options.timezone !== undefined ? { timezone: options.timezone } : {}),
      ...(entry.encoding !== undefined ? { encoding: entry.encoding } : {}),
      ...(options.filenamePattern !== undefined ? { filenamePattern: options.filenamePattern } : {}),
      ...(entry.sourceName !== undefined ? { sourceName: entry.sourceName } : {}),
    };
    const blob = await fetchExportBlob(entry.sid, exportOptions);
    const filename = renderSessionExportFilename(options.filenamePattern, {
      sid: entry.sid,
      format: options.format,
      sourceName: entry.sourceName,
      timestamp,
    });
    const path = `${folder}/${uniqueName(filename, used)}`;
    files.push({ name: path, body: new Uint8Array(await blob.arrayBuffer()) });
    entryNames.push(path);
    options.onProgress?.({
      completed: index + 1,
      total: entries.length,
      sid: entry.sid,
      sourceName: entry.sourceName,
    });
  }

  const zip = createStoredZip(files, timestamp);
  const zipBuffer = new ArrayBuffer(zip.byteLength);
  new Uint8Array(zipBuffer).set(zip);
  return {
    blob: new Blob([zipBuffer], { type: ZIP_MIME }),
    filename: sessionExportZipFilename(options.format, timestamp),
    entryNames,
  };
}

export async function downloadSessionExportZip(
  entries: SessionExportZipEntry[],
  options: SessionExportZipOptions,
): Promise<SessionExportZipResult> {
  if (entries.length === 0) throw new Error("no persisted sources to export");
  const timestamp = timestampDate(options.timestamp);
  const { ticket } = await requestSessionExportBundleTicket(entries, options, timestamp);
  const filename = sessionExportZipFilename(options.format, timestamp);
  const url = sessionExportBundleDownloadUrl(ticket);
  downloadUrl(url, filename);
  options.onProgress?.({ completed: entries.length, total: entries.length, sid: "" });
  return {
    filename,
    entryNames: serverBundleEntryNames(entries, options, timestamp),
    downloadUrl: url,
  };
}