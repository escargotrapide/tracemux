// Tile-virtualization helper for high-cardinality views (NFR-PERF-001).
// Visible tiles get their `panel_priority` raised so the server skips
// coalescing for them; off-screen tiles are coalesced (16/500/2000 ms).

import { getClient } from "~/state";

export const TILE_COUNT = 16;

export interface TileSubscription {
  sid: string;
  ch: number;
}

/**
 * Observe an element and report its visibility to the server so it can
 * pick the right coalescing bucket.
 */
export function observeVisibility(
  element: Element,
  sub: TileSubscription,
): () => void {
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        const visible = e.isIntersecting && e.intersectionRatio > 0.05;
        getClient().send({
          type: "panel_priority",
          sid: sub.sid,
          ch: sub.ch,
          payload: {
            visible,
            ratio: e.intersectionRatio,
          },
        });
      }
    },
    { threshold: [0, 0.05, 0.5, 1] },
  );
  io.observe(element);
  return () => io.disconnect();
}
