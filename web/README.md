# wanlogger web UI

SolidJS + xterm.js + Dockview. Talks WSS subprotocol `wanlogger.v1`
(MessagePack) to a running `wanlogger serve`.

## Develop

```bash
pnpm install
pnpm --filter ./web dev
```

Set `VITE_WANLOGGER_URL` and (optionally) `VITE_WANLOGGER_TOKEN` to
point at a non-default backend. Otherwise the page connects to
`wss://<page-host>/ws`.

## Layout

- `src/adapters/wss.ts` ? `WireClient`, framing, reconnect.
- `src/state/` ? global stores. **Server is the source of truth.**
- `src/state/visibility.ts` ? `IntersectionObserver` → `panel_priority`.
- `src/panels/<name>/` ? Dockview panels.
- `src/i18n/{ja,en}.json` ? i18n strings (UI defaults to JA on JA locale).

## Tests

- Unit: `pnpm --filter ./web test`
- E2E: `pnpm --filter ./web e2e` (or `just e2e`)
