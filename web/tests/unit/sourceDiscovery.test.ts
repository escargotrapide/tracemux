import { describe, expect, it, vi } from "vitest";
import { detectSources, serialSpecForPort } from "../../src/state/sourceDiscovery";

describe("source discovery", () => {
  it("normalizes detect API results", async () => {
    // REQ: FR-UI-016
    const fetchImpl = vi.fn(async () => new Response(
      JSON.stringify({
        kinds: ["serial", 1, "tcp"],
        serial_candidates: ["COM7", 42, "COM3"],
      }),
      { status: 200 },
    ));

    await expect(detectSources(fetchImpl)).resolves.toEqual({
      kinds: ["serial", "tcp"],
      serial_candidates: ["COM3", "COM7"],
    });
  });

  it("rejects failed detect responses", async () => {
    const fetchImpl = vi.fn(async () => new Response("nope", { status: 503 }));

    await expect(detectSources(fetchImpl)).rejects.toThrow(/HTTP 503/);
  });

  it("builds serial source specs", () => {
    // REQ: FR-UI-016
    expect(serialSpecForPort("COM7", { baud: 9_600 })).toBe(
      "serial://COM7?baud=9600&data=8&parity=none&stop=1&flow=none",
    );
    expect(serialSpecForPort("/dev/ttyUSB0")).toBe(
      "serial:///dev/ttyUSB0?baud=115200&data=8&parity=none&stop=1&flow=none",
    );
  });
});
