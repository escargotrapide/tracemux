import { afterEach, describe, expect, it, vi } from "vitest";
// REQ: FR-EXP-PCAPNG
import {
  fetchSessionExportBlob,
  renderSessionExportFilename,
  sessionExportDownloadUrl,
  sessionExportFilename,
  sessionExportUrl,
} from "../../src/adapters/sessionExport";

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

describe("sessionExport", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("builds download URLs with format, timezone, and encoding", () => {
    // REQ: FR-EXP-001
    stubLocation();
    expect(
      sessionExportUrl("abc", {
        format: "jsonl",
        timezone: "GMT+9",
        encoding: "shift_jis",
        filenamePattern: "{source}.{ext}",
        sourceName: "COM7",
      }),
    ).toBe(
      "http://127.0.0.1:9000/api/sessions/abc/export?format=jsonl&tz=GMT%2B9&encoding=shift_jis&filename=COM7.jsonl",
    );
  });

  it("uses stable file extensions", () => {
    expect(sessionExportFilename("sid", "text")).toBe("tracemux-sid.txt");
    expect(sessionExportFilename("sid", "csv")).toBe("tracemux-sid.csv");
    expect(sessionExportFilename("sid", "jsonl")).toBe("tracemux-sid.jsonl");
    expect(sessionExportFilename("sid", "pcapng")).toBe("tracemux-sid.pcapng");
  });

  it("renders safe custom download filenames", () => {
    // REQ: FR-EXP-001
    expect(
      renderSessionExportFilename("{source}_{timestamp}.{ext}", {
        sid: "sid",
        format: "text",
        sourceName: "serial:COM7/motor",
        timestamp: "2026-05-20T01:02:03Z",
      }),
    ).toBe("serial-COM7-motor_2026-05-20T010203Z.txt");

    expect(
      renderSessionExportFilename("{source}_{format}", {
        sid: "sid",
        format: "csv",
        sourceName: "COM7",
      }),
    ).toMatch(/^COM7_csv\.csv$/);
  });

  it("fetches export blobs through the authenticated HTTP export URL", async () => {
    // REQ: FR-UI-018
    stubLocation();
    const fetchMock = vi.fn(async () => new Response("export body", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const blob = await fetchSessionExportBlob("sid-fetch", { format: "text" });

    expect(await blob.text()).toBe("export body");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:9000/api/sessions/sid-fetch/export?format=text&filename=tracemux-sid-fetch.txt",
      { headers: {} },
    );
  });

  it("adds a one-time ticket to native download URLs when auth is configured", async () => {
    // REQ: FR-UI-018
    stubLocation();
    vi.stubEnv("VITE_TRACEMUX_TOKEN", "secret-token");
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      ticket: "ticket-1",
      expires_in_ms: 60_000,
      expires_at_ms: 1_780_134_200_000,
    }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const url = await sessionExportDownloadUrl("sid-ticket", {
      format: "pcapng",
      filenamePattern: "{source}.{ext}",
      sourceName: "Loopback",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:9000/api/sessions/sid-ticket/export-ticket?format=pcapng",
      {
        method: "POST",
        headers: { Authorization: "Bearer secret-token" },
      },
    );
    expect(url).toBe(
      "http://127.0.0.1:9000/api/sessions/sid-ticket/export?format=pcapng&filename=Loopback.pcapng&ticket=ticket-1",
    );
  });
});
