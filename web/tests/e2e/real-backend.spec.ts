// GUI real-backend smoke: drives the LIVE UI against a real `tracemux serve`
// and `tracemux-virt-peer` (spawned by global setup). Unlike shell.spec.ts,
// this exercises the actual browser WireClient -> WSS -> source path with no
// injection/spy client.
//
// REQ: FR-WIRE-001
// REQ: FR-LOG-001
// REQ: FR-UI-001

import { test, expect, type Page } from "@playwright/test";
import { PEER_HOST, PEER_PORT, PEER_SEND_TEXT } from "./realBackend.harness";

interface RealApi {
  connStatus: () => string;
  sources: () => Array<{ sid: string; kind: string; bytesIn: number }>;
  sendCtl: (
    sid: string | undefined,
    action: string,
    spec?: Record<string, unknown>,
    options?: Record<string, unknown>,
  ) => boolean;
  subscribe: (sid: string, ch: number, cb: (p: unknown) => void) => () => void;
  openTerminal: (sid: string, ch: number) => void;
}

function realApi(page: Page) {
  return {
    connStatus: () =>
      page.evaluate(
        () => (window as unknown as { __tracemuxRealApi: RealApi }).__tracemuxRealApi.connStatus(),
      ),
    sources: () =>
      page.evaluate(
        () => (window as unknown as { __tracemuxRealApi: RealApi }).__tracemuxRealApi.sources(),
      ),
    startTcp: (addr: string) =>
      page.evaluate(
        (a) =>
          (window as unknown as { __tracemuxRealApi: RealApi }).__tracemuxRealApi.sendCtl(
            undefined,
            "start",
            { kind: "tcp", addr: a },
            {},
          ),
        addr,
      ),
    openTerminal: (sid: string, ch: number) =>
      page.evaluate(
        ([s, c]) =>
          (window as unknown as { __tracemuxRealApi: RealApi }).__tracemuxRealApi.openTerminal(
            s as string,
            c as number,
          ),
        [sid, ch] as const,
      ),
  };
}

test("UI connects to a real server and renders live TCP source data", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(
    () => typeof (window as unknown as { __tracemuxRealApi?: unknown }).__tracemuxRealApi === "object",
  );

  const api = realApi(page);

  // 1. The live WireClient completed the hello handshake against the real server.
  await expect.poll(() => api.connStatus(), { timeout: 15_000 }).toBe("open");

  // 2. Start a TCP source pointed at the virtual peer through the real ctl path.
  expect(await api.startTcp(`${PEER_HOST}:${PEER_PORT}`)).toBe(true);

  // 3. The server creates the source and syncs it back to the UI store.
  const sid = await page
    .waitForFunction(
      () => {
        const found = (window as unknown as { __tracemuxRealApi: RealApi }).__tracemuxRealApi
          .sources()
          .find((s) => s.kind === "tcp");
        return found ? found.sid : null;
      },
      { timeout: 15_000 },
    )
    .then((handle) => handle.jsonValue() as Promise<string>);
  expect(sid).toBeTruthy();

  // 4. Opening the terminal subscribes through the real GUI path; live bytes
  //    from the peer must render in the xterm view (DOM renderer rows).
  await api.openTerminal(sid, 0);
  await expect
    .poll(
      () => page.locator(".xterm-rows").filter({ hasText: PEER_SEND_TEXT }).count(),
      { timeout: 20_000 },
    )
    .toBeGreaterThan(0);

  // 5. The per-source aggregate reflects received bytes.
  await expect
    .poll(
      async () => {
        const found = (await api.sources()).find((s) => s.sid === sid);
        return found ? found.bytesIn : 0;
      },
      { timeout: 20_000 },
    )
    .toBeGreaterThan(0);
});
