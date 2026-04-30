import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const send = vi.fn();

vi.mock("~/state", () => ({
  getClient: () => ({ send }),
}));

import { observeVisibility, TILE_COUNT } from "../../src/state/visibility";

let latestObserver: FakeIntersectionObserver | undefined;

class FakeIntersectionObserver {
  readonly observe = vi.fn();
  readonly disconnect = vi.fn();

  constructor(
    private readonly callback: IntersectionObserverCallback,
    readonly options?: IntersectionObserverInit,
  ) {
    latestObserver = this;
  }

  emit(entry: Partial<IntersectionObserverEntry>): void {
    this.callback(
      [entry as IntersectionObserverEntry],
      this as unknown as IntersectionObserver,
    );
  }
}

describe("visibility reporting", () => {
  beforeEach(() => {
    send.mockReset();
    latestObserver = undefined;
    vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reports panel priority from IntersectionObserver", () => {
    // REQ: FR-UI-004
    // REQ: NFR-PERF-001
    const element = document.createElement("div");

    const stop = observeVisibility(element, { sid: "sid-visible", ch: 1 });

    expect(latestObserver?.observe).toHaveBeenCalledWith(element);
    latestObserver?.emit({ isIntersecting: true, intersectionRatio: 0.5 });
    expect(send).toHaveBeenCalledWith({
      type: "panel_priority",
      sid: "sid-visible",
      ch: 1,
      payload: { visible: true, ratio: 0.5 },
    });

    latestObserver?.emit({ isIntersecting: false, intersectionRatio: 0 });
    expect(send).toHaveBeenCalledWith({
      type: "panel_priority",
      sid: "sid-visible",
      ch: 1,
      payload: { visible: false, ratio: 0 },
    });

    stop();
    expect(latestObserver?.disconnect).toHaveBeenCalled();
  });

  it("pins the tile virtualization window to sixteen tiles", () => {
    // REQ: FR-UI-012
    // REQ: NFR-PERF-001
    expect(TILE_COUNT).toBe(16);
  });
});
