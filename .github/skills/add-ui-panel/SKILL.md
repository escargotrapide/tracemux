---
name: add-ui-panel
description: Add a Dockview panel to the SolidJS web UI
---

# Skill: add a UI panel

The web UI is SolidJS + xterm.js (WebGL addon ON) + Dockview. Every
panel is a self-contained component subscribing to the WSS data stream
through `web/src/adapters/`.

## Steps

1. Add the panel under `web/src/panels/<name>/`. Export a Dockview
   panel descriptor.
2. Subscribe via `useChannel(sid, ch)` from `web/src/state/`.
   Honour the panel-priority API: hidden panels are coalesced
   (16 ms / 500 ms / 2 s) by the server. Use `IntersectionObserver`.
3. For high-cardinality views (100-1000 sources) virtualize via
   `web/src/state/visibility.ts` tile lists (N=16).
4. Add i18n keys under `web/src/i18n/{ja,en}.json`.
5. Playwright e2e under `tests/e2e/<name>.spec.ts` driving a
   `MockSource`.
6. `FR-UI-<name>` in requirements.
7. `just ai-verify` + `just e2e`.

## Pitfalls

- xterm.js: keep WebGL addon enabled; CPU rendering is the fallback.
- Never persist panel state outside `web/src/state/`. The server is the
  source of truth.
