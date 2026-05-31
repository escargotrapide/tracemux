import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createSessionExportZip,
  createStoredZip,
  downloadSessionExportZip,
  sessionExportZipFilename,
} from "../../src/adapters/sessionExportZip";

function stubLocation(): void {
  vi.stubGlobal("window", {
    location: {
      protocol: "http:",
      hostname: "127.0.0.1",
      host: "127.0.0.1:5173",
      port: "5173",
    },
  });
}

describe("sessionExportZip", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("names all-sources ZIP downloads with a stable timestamp token", () => {
    // REQ: FR-UI-018
    expect(sessionExportZipFilename("jsonl", "2026-05-20T01:02:03Z"))
      .toBe("tracemux-all-2026-05-20T010203Z-jsonl.zip");
  });

  it("creates a stored ZIP containing one export per source", async () => {
    // REQ: FR-UI-018
    const fetchExportBlob = vi.fn(async (sid: string) => new Blob([`body:${sid}`]));

    const result = await createSessionExportZip([
      { sid: "sid-a", sourceName: "COM7" },
      { sid: "sid-b", sourceName: "COM7" },
    ], {
      format: "text",
      filenamePattern: "{source}.{ext}",
      timestamp: "2026-05-20T01:02:03Z",
      fetchExportBlob,
    });

    expect(result.filename).toBe("tracemux-all-2026-05-20T010203Z-text.zip");
    expect(result.entryNames).toEqual([
      "tracemux-all-2026-05-20T010203Z-text/COM7.txt",
      "tracemux-all-2026-05-20T010203Z-text/COM7-2.txt",
    ]);
    expect(fetchExportBlob).toHaveBeenCalledTimes(2);
    expect(result.blob).toBeDefined();
    const zipText = new TextDecoder().decode(await result.blob!.arrayBuffer());
    expect(zipText).toContain("tracemux-all-2026-05-20T010203Z-text/COM7.txt");
    expect(zipText).toContain("tracemux-all-2026-05-20T010203Z-text/COM7-2.txt");
    expect(zipText).toContain("body:sid-a");
    expect(zipText).toContain("body:sid-b");
  });

  it("requests a server-side bundle ticket for bulk downloads", async () => {
    // REQ: FR-UI-018
    stubLocation();
    vi.stubEnv("VITE_TRACEMUX_TOKEN", "secret-token");
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      ticket: "bundle-ticket",
      expires_in_ms: 60_000,
      expires_at_ms: 1_780_134_200_000,
    }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const clicked: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function click() {
      clicked.push(this.href);
    });

    const result = await downloadSessionExportZip([
      { sid: "sid-a", sourceName: "Loopback", encoding: "utf-8" },
      { sid: "sid-b", sourceName: "Wi-Fi" },
    ], {
      format: "pcapng",
      timezone: "UTC",
      filenamePattern: "{source}.{ext}",
      timestamp: "2026-05-20T01:02:03Z",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:9000/api/exports/bundle-ticket",
      expect.objectContaining({
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: "Bearer secret-token",
        },
      }),
    );
    const body = JSON.parse((fetchMock.mock.calls[0]?.[1] as RequestInit).body as string);
    expect(body).toEqual({
      entries: [
        { sid: "sid-a", source_name: "Loopback", encoding: "utf-8" },
        { sid: "sid-b", source_name: "Wi-Fi" },
      ],
      format: "pcapng",
      tz: "UTC",
      filename_pattern: "{source}.{ext}",
      timestamp_ms: Date.parse("2026-05-20T01:02:03Z"),
    });
    expect(result.filename).toBe("tracemux-all-2026-05-20T010203Z-pcapng.zip");
    expect(result.entryNames).toEqual([
      "tracemux-all-2026-05-20T010203Z-pcapng/Loopback.pcapng",
      "tracemux-all-2026-05-20T010203Z-pcapng/Wi-Fi.pcapng",
    ]);
    expect(result.downloadUrl).toBe(
      "http://127.0.0.1:9000/api/exports/bundle?ticket=bundle-ticket",
    );
    expect(clicked).toEqual([
      "http://127.0.0.1:9000/api/exports/bundle?ticket=bundle-ticket",
    ]);
  });

  it("writes ZIP local and end-of-central-directory signatures", () => {
    // REQ: FR-UI-018
    const zip = createStoredZip([
      { name: "one.txt", body: new TextEncoder().encode("one") },
    ], new Date("2026-05-20T01:02:03Z"));

    expect([...zip.slice(0, 4)]).toEqual([0x50, 0x4b, 0x03, 0x04]);
    expect([...zip.slice(-22, -18)]).toEqual([0x50, 0x4b, 0x05, 0x06]);
    expect(new TextDecoder().decode(zip)).toContain("one.txt");
  });
});