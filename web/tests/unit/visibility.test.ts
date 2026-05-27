import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const send = vi.fn();

vi.mock("~/state", () => ({
  getClient: () => ({ send }),
}));

import { observeVisibility, TILE_COUNT } from "../../src/state/visibility";

let latestObserver: FakeIntersectionObserver | undefined;
let rafCallbacks: FrameRequestCallback[] = [];

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

function flushAnimationFrame(): void {
  const callbacks = rafCallbacks;
  rafCallbacks = [];
  for (const callback of callbacks) callback(performance.now());
}

describe("visibility reporting", () => {
  beforeEach(() => {
    send.mockReset();
    latestObserver = undefined;
    vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
    rafCallbacks = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      rafCallbacks.push(callback);
      return rafCallbacks.length;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
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
    flushAnimationFrame();
    expect(send).toHaveBeenCalledWith({
      type: "panel_priority",
      sid: "sid-visible",
      ch: 1,
      payload: { visible: true, ratio: 0.5 },
    });

    latestObserver?.emit({ isIntersecting: false, intersectionRatio: 0 });
    flushAnimationFrame();
    expect(send).toHaveBeenCalledWith({
      type: "panel_priority",
      sid: "sid-visible",
      ch: 1,
      payload: { visible: false, ratio: 0 },
    });

    stop();
    expect(latestObserver?.disconnect).toHaveBeenCalled();
  });

  it("coalesces rapid panel priority updates to the latest frame", () => {
    // REQ: FR-UI-004
    // REQ: NFR-PERF-001
    const element = document.createElement("div");

    observeVisibility(element, { sid: "sid-rapid", ch: 2 });
    latestObserver?.emit({ isIntersecting: true, intersectionRatio: 0.5 });
    latestObserver?.emit({ isIntersecting: false, intersectionRatio: 0 });

    expect(send).not.toHaveBeenCalled();
    flushAnimationFrame();

    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith({
      type: "panel_priority",
      sid: "sid-rapid",
      ch: 2,
      payload: { visible: false, ratio: 0 },
    });
  });

  it("pins the tile virtualization window to sixteen tiles", () => {
    // REQ: FR-UI-012
    // REQ: NFR-PERF-001
    expect(TILE_COUNT).toBe(16);
  });
});
