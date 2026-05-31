import type { DataPayload } from "~/adapters/wss";

const buffers = new Map<string, DataPayload[]>();

export function channelKey(sid: string, ch: number): string {
  return `${sid}/${ch}`;
}

function normalizedLimit(maxRecords: number): number {
  if (!Number.isFinite(maxRecords)) return 0;
  return Math.max(0, Math.trunc(maxRecords));
}

export function appendChannelFrame(payload: DataPayload, maxRecords: number): void {
  const limit = normalizedLimit(maxRecords);
  if (limit === 0) return;
  const key = channelKey(payload.sid, payload.ch);
  const list = buffers.get(key) ?? [];
  list.push(payload);
  if (list.length > limit) {
    list.splice(0, list.length - limit);
  }
  buffers.set(key, list);
}

export function getChannelFrames(
  sid: string,
  ch: number,
  maxRecords?: number,
): DataPayload[] {
  const list = buffers.get(channelKey(sid, ch)) ?? [];
  if (maxRecords === undefined) return [...list];
  const limit = normalizedLimit(maxRecords);
  if (limit === 0) return [];
  return list.slice(-limit);
}

export function clearChannelFrames(sid?: string, ch?: number): void {
  if (sid === undefined) {
    buffers.clear();
    return;
  }
  if (ch !== undefined) {
    buffers.delete(channelKey(sid, ch));
    return;
  }
  const prefix = `${sid}/`;
  for (const key of [...buffers.keys()]) {
    if (key.startsWith(prefix)) buffers.delete(key);
  }
}

export function __resetChannelBuffersForTest(): void {
  clearChannelFrames();
}
