// Tile-virtualization helper for high-cardinality views (NFR-PERF-001).
// Visible tiles get their `panel_priority` raised so the server skips
// coalescing for them; off-screen tiles are coalesced (16/500/2000 ms).

import { getClient } from "~/state";

export const TILE_COUNT = 16;

export interface TileSubscription {
  sid: string;
  ch: number;
}

interface PriorityPayload {
  visible: boolean;
  ratio: number;
}

function schedulePriorityFlush(callback: FrameRequestCallback): number {
  if (typeof requestAnimationFrame === "function") return requestAnimationFrame(callback);
  return window.setTimeout(() => callback(performance.now()), 16);
}

function cancelPriorityFlush(handle: number): void {
  if (typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(handle);
    return;
  }
  clearTimeout(handle);
}

/**
 * Observe an element and report its visibility to the server so it can
 * pick the right coalescing bucket.
 */
export function observeVisibility(
  element: Element,
  sub: TileSubscription,
): () => void {
  let pending: PriorityPayload | null = null;
  let flushHandle: number | null = null;

  const flush = () => {
    flushHandle = null;
    const payload = pending;
    pending = null;
    if (!payload) return;
    getClient().send({
      type: "panel_priority",
      sid: sub.sid,
      ch: sub.ch,
      payload,
    });
  };

  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        pending = {
          visible: e.isIntersecting && e.intersectionRatio > 0.05,
          ratio: e.intersectionRatio,
        };
      }
      if (flushHandle === null) flushHandle = schedulePriorityFlush(flush);
    },
    { threshold: [0, 0.05, 0.5, 1] },
  );
  io.observe(element);
  return () => {
    io.disconnect();
    if (flushHandle !== null) cancelPriorityFlush(flushHandle);
    pending = null;
    flushHandle = null;
  };
}
