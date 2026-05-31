# tracemux web UI

SolidJS + xterm.js + Dockview. Talks WSS subprotocol `tracemux.v1`
(MessagePack) to a running `tracemux serve`.

## Develop

```bash
pnpm install
pnpm --filter ./web dev
```

Set `VITE_TRACEMUX_URL` and (optionally) `VITE_TRACEMUX_TOKEN` to
point at a non-default backend. Otherwise the page connects to
`ws://127.0.0.1:9000/ws` when running under the Vite dev server or
Tauri custom protocol, and to `ws(s)://<page-host>/ws` when served from
an HTTP(S) origin.

## Layout

- `src/adapters/wss.ts` — `WireClient`, framing, reconnect, endpoint resolution.
- `src/state/` — global stores. **Server is the source of truth.**
- `src/state/sourceSpec.ts` — URI-style source spec parser.
- `src/state/sourcePresets.ts` — browser-local source presets/profiles
	(source specs only; never log data).
- `src/state/sourceFilters.ts` — source search/filter/sort helper.
- `src/state/visibility.ts` — `IntersectionObserver` -> `panel_priority`.
- `src/panels/<name>/` — Dockview panels.
- `src/i18n/{ja,en}.json` — i18n strings (UI defaults to JA on JA locale).

## Source and terminal workflow

The Sources panel can start server-side sources from URI-style specs,
for example:

- `mock://demo`
- `file:///C:/logs/app.log?follow=1`
- `tcp://127.0.0.1:5555`
- `serial://COM3?baud=115200&data=8&parity=none&stop=1&flow=none`

Lifecycle buttons send WSS `ctl` actions (`start`, `stop`, `restart`,
`remove`) and the UI re-requests `list` so the table converges to the
server's source registry. The terminal panel follows the globally
selected `(sid, ch)` target and does not subscribe until a real source
exists.

## Local UI metrics

The Metrics panel always shows a local `ui.*` section alongside server
metrics. These counters cover received frames, data/ctl/metrics frame
counts, source table updates, source list syncs, subscription dispatches,
active subscriptions, and bounded-toast drops.

## Tests

- Unit: `pnpm --filter ./web test`
- E2E: `pnpm --filter ./web e2e` (or `just e2e`)
