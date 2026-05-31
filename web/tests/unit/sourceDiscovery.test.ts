import { describe, expect, it, vi } from "vitest";
// REQ: FR-SRC-PCAP-DETECT
// REQ: FR-UI-PCAP
// REQ: NFR-SEC-PCAP
import { detectSources, pcapSpecForInterface, serialSpecForPort } from "../../src/state/sourceDiscovery";

describe("source discovery", () => {
  it("normalizes detect API results", async () => {
    // REQ: FR-UI-016
    const fetchImpl = vi.fn(async () => new Response(
      JSON.stringify({
        kinds: ["serial", 1, "tcp"],
        serial_candidates: ["COM7", 42, "COM3"],
        pcap_interfaces: [
          { device: "eth1", display_name: "Zeta", addresses: ["10.0.0.2", 5], flags: ["up"] },
          { device: "eth0", display_name: "Alpha", description: "primary", flags: ["up", "loopback"] },
          { device: "eth0", display_name: "Duplicate" },
          { display_name: "missing device" },
        ],
      }),
      { status: 200 },
    ));

    await expect(detectSources(fetchImpl)).resolves.toEqual({
      kinds: ["serial", "tcp"],
      serial_candidates: ["COM3", "COM7"],
      pcap_interfaces: [
        {
          device: "eth0",
          display_name: "Alpha",
          description: "primary",
          addresses: [],
          flags: ["loopback", "up"],
        },
        {
          device: "eth1",
          display_name: "Zeta",
          addresses: ["10.0.0.2"],
          flags: ["up"],
        },
      ],
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

  it("builds pcap source specs", () => {
    expect(
      pcapSpecForInterface(
        { device: "Ethernet 0", display_name: "Corp LAN", addresses: [], flags: [] },
        { snaplen: 9000, promiscuous: true, filter: "tcp port 502", publishMode: "sampled" },
      ),
    ).toBe(
      "pcap://Ethernet%200?snaplen=9000&promisc=1&save=session&publish=sampled&display_name=Corp+LAN&filter=tcp+port+502",
    );
  });
});
