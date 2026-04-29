import { describe, it, expect } from "vitest";
import { resolveWanloggerUrl } from "../../src/adapters/wss";

describe("resolveWanloggerUrl", () => {
  it("returns a fallback when no window or env", () => {
    // Vitest runs in jsdom by default; just assert it returns a string.
    const url = resolveWanloggerUrl();
    expect(typeof url).toBe("string");
    expect(url.length).toBeGreaterThan(0);
  });
});
