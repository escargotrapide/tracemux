import type { DataPayload } from "~/adapters/wss";

// REQ: FR-UI-PCAP
// REQ: NFR-PERF-PCAP

export const DEFAULT_PACKET_RING_CAPACITY = 512;
export const DEFAULT_HEX_PREVIEW_BYTES = 256;

export interface PacketCaptureEntry {
  id: number;
  sid: string;
  ch: number;
  tsOrigin: bigint | number;
  tsIngest: bigint | number;
  source?: string;
  capturedLen: number;
  originalLen: number;
  bytes: Uint8Array;
}

export interface HexPreviewRow {
  offset: string;
  hex: string;
  ascii: string;
}

export function packetFromDataPayload(
  payload: DataPayload,
  id: number,
): PacketCaptureEntry | null {
  if (payload.kind !== "datagram" || !(payload.body instanceof Uint8Array)) {
    return null;
  }
  const capturedLen = payload.body.byteLength;
  const packet: PacketCaptureEntry = {
    id,
    sid: payload.sid,
    ch: payload.ch,
    tsOrigin: payload.ts_origin,
    tsIngest: payload.ts_ingest,
    capturedLen,
    originalLen: capturedLen,
    bytes: payload.body,
  };
  if (payload.source) packet.source = payload.source;
  return packet;
}

export function appendPacketRing(
  packets: readonly PacketCaptureEntry[],
  packet: PacketCaptureEntry,
  capacity = DEFAULT_PACKET_RING_CAPACITY,
): PacketCaptureEntry[] {
  const safeCapacity = Math.max(1, Math.floor(capacity));
  const next = [...packets, packet];
  return next.length > safeCapacity ? next.slice(next.length - safeCapacity) : next;
}

export function packetProtocolHint(packet: Pick<PacketCaptureEntry, "bytes">): string {
  const bytes = packet.bytes;
  if (bytes.byteLength < 14) return "datagram";
  const ethertype = (bytes[12]! << 8) | bytes[13]!;
  if (ethertype === 0x0800) return "ipv4";
  if (ethertype === 0x86dd) return "ipv6";
  if (ethertype === 0x0806) return "arp";
  return `ethertype:0x${ethertype.toString(16).padStart(4, "0")}`;
}

export function formatPacketTimestamp(value: bigint | number): string {
  const ns = typeof value === "bigint" ? value : BigInt(Math.trunc(value));
  const ms = ns / 1_000_000n;
  const date = new Date(Number(ms));
  if (Number.isNaN(date.getTime())) return String(value);
  return date.toISOString();
}

export function hexPreview(
  bytes: Uint8Array,
  maxBytes = DEFAULT_HEX_PREVIEW_BYTES,
  width = 16,
): HexPreviewRow[] {
  const safeWidth = Math.max(1, Math.floor(width));
  const limit = Math.min(bytes.byteLength, Math.max(0, Math.floor(maxBytes)));
  const rows: HexPreviewRow[] = [];
  for (let offset = 0; offset < limit; offset += safeWidth) {
    const slice = bytes.slice(offset, Math.min(offset + safeWidth, limit));
    const hex = Array.from(slice, (byte) => byte.toString(16).padStart(2, "0")).join(" ");
    const ascii = Array.from(slice, (byte) => {
      if (byte >= 0x20 && byte <= 0x7e) return String.fromCharCode(byte);
      return ".";
    }).join("");
    rows.push({
      offset: offset.toString(16).padStart(8, "0"),
      hex,
      ascii,
    });
  }
  return rows;
}
