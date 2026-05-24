import { describe, expect, it } from "vitest";
// REQ: FR-UI-PCAP
// REQ: NFR-PERF-PCAP
import type { DataPayload } from "../../src/adapters/wss";
import {
  appendPacketRing,
  formatPacketTimestamp,
  hexPreview,
  packetFromDataPayload,
  packetProtocolHint,
} from "../../src/state/packetCapture";

function packetPayload(body: Uint8Array, kind: DataPayload["kind"] = "datagram"): DataPayload {
  return {
    ts_origin: 1_700_000_000_123_456_789n,
    ts_ingest: 1_700_000_000_223_456_789n,
    mono_ns: 42,
    boot_id: "00000000-0000-0000-0000-000000000000",
    node_id: "00000000-0000-0000-0000-000000000000",
    clock_offset_ms: 0,
    clock_quality: "imported",
    drift_ppm: 0,
    clock_source: "imported",
    sid: "11111111-1111-4111-8111-111111111111",
    ch: 0,
    dir: "in",
    kind,
    body,
    source: "pcap:eth0",
  };
}

describe("packetCapture state helpers", () => {
  it("accepts datagram payloads and rejects non-packet frames", () => {
    const packet = packetFromDataPayload(packetPayload(new Uint8Array([1, 2, 3])), 7);

    expect(packet?.id).toBe(7);
    expect(packet?.capturedLen).toBe(3);
    expect(packet?.originalLen).toBe(3);
    expect(packet?.source).toBe("pcap:eth0");
    expect(packetFromDataPayload(packetPayload(new Uint8Array([1]), "bytes"), 1)).toBeNull();
  });

  it("keeps only the bounded ring tail", () => {
    const packets = [1, 2, 3, 4].reduce((acc, id) => {
      const packet = packetFromDataPayload(packetPayload(new Uint8Array([id])), id)!;
      return appendPacketRing(acc, packet, 2);
    }, [] as ReturnType<typeof appendPacketRing>);

    expect(packets.map((packet) => packet.id)).toEqual([3, 4]);
  });

  it("renders protocol hints and hex preview", () => {
    const bytes = new Uint8Array([
      0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
      0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
      0x08, 0x00,
      0x41, 0x00,
    ]);
    const packet = packetFromDataPayload(packetPayload(bytes), 1)!;

    expect(packetProtocolHint(packet)).toBe("ipv4");
    expect(hexPreview(bytes, 16, 8)).toEqual([
      { offset: "00000000", hex: "aa bb cc dd ee ff 00 11", ascii: "........" },
      { offset: "00000008", hex: "22 33 44 55 08 00 41 00", ascii: "\"3DU..A." },
    ]);
  });

  it("formats nanosecond timestamps", () => {
    expect(formatPacketTimestamp(1_700_000_000_123_456_789n)).toContain("2023-");
  });
});
