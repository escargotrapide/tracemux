import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveWanloggerHttpUrl, resolveWanloggerUrl } from "../../src/adapters/wss";

function stubLocation(location: Partial<Location>): void {
  vi.stubGlobal("window", {
    location: {
      protocol: "http:",
      hostname: "127.0.0.1",
      host: "127.0.0.1:5173",
      port: "5173",
      ...location,
    },
  });
}

describe("resolveWanloggerUrl", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("honors VITE_WANLOGGER_URL", () => {
    vi.stubEnv("VITE_WANLOGGER_URL", "ws://example.test/ws");
    expect(resolveWanloggerUrl()).toBe("ws://example.test/ws");
  });

  it("uses the loopback backend for the Vite dev server", () => {
    stubLocation({
      protocol: "http:",
      hostname: "127.0.0.1",
      host: "127.0.0.1:5173",
      port: "5173",
    });
    expect(resolveWanloggerUrl()).toBe("ws://127.0.0.1:9000/ws");
    expect(resolveWanloggerHttpUrl("/api/detect")).toBe(
      "http://127.0.0.1:9000/api/detect",
    );
  });

  it("uses the page host for deployed HTTP origins", () => {
    stubLocation({
      protocol: "https:",
      hostname: "logs.example.test",
      host: "logs.example.test",
      port: "",
    });
    expect(resolveWanloggerUrl()).toBe("wss://logs.example.test/ws");
    expect(resolveWanloggerHttpUrl("/api/version")).toBe(
      "https://logs.example.test/api/version",
    );
  });

  it("uses the loopback backend for Tauri custom protocols", () => {
    stubLocation({
      protocol: "tauri:",
      hostname: "localhost",
      host: "localhost",
      port: "",
    });
    expect(resolveWanloggerUrl()).toBe("ws://127.0.0.1:9000/ws");
  });
});
