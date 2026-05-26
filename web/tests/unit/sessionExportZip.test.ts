import { describe, expect, it, vi } from "vitest";
import {
  createSessionExportZip,
  createStoredZip,
  sessionExportZipFilename,
} from "../../src/adapters/sessionExportZip";

describe("sessionExportZip", () => {
  it("names all-sources ZIP downloads with a stable timestamp token", () => {
    // REQ: FR-UI-018
    expect(sessionExportZipFilename("jsonl", "2026-05-20T01:02:03Z"))
      .toBe("wanlogger-all-2026-05-20T010203Z-jsonl.zip");
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

    expect(result.filename).toBe("wanlogger-all-2026-05-20T010203Z-text.zip");
    expect(result.entryNames).toEqual([
      "wanlogger-all-2026-05-20T010203Z-text/COM7.txt",
      "wanlogger-all-2026-05-20T010203Z-text/COM7-2.txt",
    ]);
    expect(fetchExportBlob).toHaveBeenCalledTimes(2);
    const zipText = new TextDecoder().decode(await result.blob.arrayBuffer());
    expect(zipText).toContain("wanlogger-all-2026-05-20T010203Z-text/COM7.txt");
    expect(zipText).toContain("wanlogger-all-2026-05-20T010203Z-text/COM7-2.txt");
    expect(zipText).toContain("body:sid-a");
    expect(zipText).toContain("body:sid-b");
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