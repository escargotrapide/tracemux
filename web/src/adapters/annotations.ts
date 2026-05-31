import { resolveTraceMuxHttpUrl, resolveTraceMuxToken } from "~/adapters/wss";

export type AnnotationTargetKind = "session" | "log_type";

export interface ServerAnnotationTarget {
  kind: AnnotationTargetKind;
  sid?: string;
  key?: string;
}

export interface ServerAnnotation {
  id: string;
  target: ServerAnnotationTarget;
  text: string;
  updated_at: string;
  updated_by?: string;
  deleted: boolean;
}

export interface ServerAnnotationUpsert {
  target: ServerAnnotationTarget;
  text: string;
  updated_by?: string;
}

export interface ListServerAnnotationsOptions {
  sid?: string;
}

export type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function authHeaders(json = false): HeadersInit {
  const headers: Record<string, string> = {};
  const token = resolveTraceMuxToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (json) headers["Content-Type"] = "application/json";
  return headers;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object";
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function normalizeTarget(value: unknown): ServerAnnotationTarget | null {
  if (!isObject(value)) return null;
  const kind = asString(value.kind);
  if (kind !== "session" && kind !== "log_type") return null;
  const target: ServerAnnotationTarget = { kind };
  const sid = asString(value.sid)?.trim();
  const key = asString(value.key)?.trim();
  if (sid) target.sid = sid;
  if (key) target.key = key;
  if (kind === "session" && !target.sid) return null;
  if (kind === "log_type" && !target.key) return null;
  return target;
}

export function normalizeServerAnnotation(value: unknown): ServerAnnotation | null {
  if (!isObject(value)) return null;
  const id = asString(value.id)?.trim();
  const target = normalizeTarget(value.target);
  const text = asString(value.text) ?? "";
  const updatedAt = asString(value.updated_at)?.trim();
  if (!id || !UUID_RE.test(id) || !target || !updatedAt) return null;
  const updatedBy = asString(value.updated_by)?.trim();
  const annotation: ServerAnnotation = {
    id,
    target,
    text,
    updated_at: updatedAt,
    deleted: value.deleted === true,
  };
  if (updatedBy) annotation.updated_by = updatedBy;
  return annotation;
}

export function normalizeServerAnnotations(value: unknown): ServerAnnotation[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    const annotation = normalizeServerAnnotation(item);
    return annotation ? [annotation] : [];
  });
}

export function serverAnnotationsUrl(options: ListServerAnnotationsOptions = {}): string {
  const base = resolveTraceMuxHttpUrl("/api/annotations");
  const sid = options.sid?.trim();
  if (!sid) return base;
  const params = new URLSearchParams({ sid });
  return `${base}?${params}`;
}

async function errorMessage(response: Response, fallback: string): Promise<string> {
  const detail = await response.text().catch(() => "");
  return detail || fallback;
}

export async function listServerAnnotations(
  options: ListServerAnnotationsOptions = {},
  fetchImpl: FetchLike = fetch,
): Promise<ServerAnnotation[]> {
  const response = await fetchImpl(serverAnnotationsUrl(options), {
    headers: authHeaders(),
  });
  if (!response.ok) {
    throw new Error(await errorMessage(response, `annotations list failed: HTTP ${response.status}`));
  }
  return normalizeServerAnnotations(await response.json() as unknown);
}

export async function putServerAnnotation(
  id: string,
  request: ServerAnnotationUpsert,
  fetchImpl: FetchLike = fetch,
): Promise<ServerAnnotation> {
  const response = await fetchImpl(
    resolveTraceMuxHttpUrl(`/api/annotations/${encodeURIComponent(id)}`),
    {
      method: "PUT",
      headers: authHeaders(true),
      body: JSON.stringify(request),
    },
  );
  if (!response.ok) {
    throw new Error(await errorMessage(response, `annotation save failed: HTTP ${response.status}`));
  }
  const annotation = normalizeServerAnnotation(await response.json() as unknown);
  if (!annotation) throw new Error("annotation save returned malformed response");
  return annotation;
}

export async function deleteServerAnnotation(
  id: string,
  fetchImpl: FetchLike = fetch,
): Promise<void> {
  const response = await fetchImpl(
    resolveTraceMuxHttpUrl(`/api/annotations/${encodeURIComponent(id)}`),
    {
      method: "DELETE",
      headers: authHeaders(),
    },
  );
  if (!response.ok && response.status !== 404) {
    throw new Error(await errorMessage(response, `annotation delete failed: HTTP ${response.status}`));
  }
}

export function sessionAnnotationTarget(sid: string): ServerAnnotationTarget {
  return { kind: "session", sid: sid.trim() };
}

export function logTypeAnnotationTarget(key: string, sid?: string): ServerAnnotationTarget {
  const target: ServerAnnotationTarget = { kind: "log_type", key: key.trim() };
  const trimmedSid = sid?.trim();
  if (trimmedSid) target.sid = trimmedSid;
  return target;
}

export function annotationIdForTarget(target: ServerAnnotationTarget): string {
  const key = target.kind === "session"
    ? `session:${target.sid?.trim().toLowerCase() ?? ""}`
    : `log_type:${target.sid?.trim().toLowerCase() ?? "*"}:${target.key?.trim() ?? ""}`;
  return uuidFromString(key);
}

function uuidFromString(value: string): string {
  const bytes = new Uint8Array(16);
  const seeds = [0x811c9dc5, 0x9e3779b9, 0x85ebca6b, 0xc2b2ae35];
  for (let block = 0; block < 4; block += 1) {
    const hash = hash32(value, seeds[block] ?? 0);
    bytes[block * 4] = (hash >>> 24) & 0xff;
    bytes[block * 4 + 1] = (hash >>> 16) & 0xff;
    bytes[block * 4 + 2] = (hash >>> 8) & 0xff;
    bytes[block * 4 + 3] = hash & 0xff;
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x50;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  return formatUuid(bytes);
}

function hash32(value: string, seed: number): number {
  let hash = (0x811c9dc5 ^ seed) >>> 0;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  hash ^= value.length;
  hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b) >>> 0;
  hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35) >>> 0;
  return (hash ^ (hash >>> 16)) >>> 0;
}

function formatUuid(bytes: Uint8Array): string {
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
