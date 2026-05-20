// E2E shell test driving the UI through the dev-only
// `window.__wanloggerInject` hook (no real WSS server needed).
//
// REQ: FR-UI-001
// REQ: FR-UI-002
// REQ: FR-UI-003
// REQ: FR-UI-005
// REQ: FR-UI-008
// REQ: FR-UI-009

import { test, expect, type Page } from "@playwright/test";

async function waitForInject(page: Page): Promise<void> {
  await page.waitForFunction(
    () =>
      typeof (window as unknown as { __wanloggerInject?: unknown })
        .__wanloggerInject === "function",
  );
}

async function injectFrame(page: Page, frame: unknown): Promise<void> {
  await page.evaluate((payload) => {
    const fn = (
      window as unknown as { __wanloggerInject: (f: unknown) => void }
    ).__wanloggerInject;
    fn(payload);
  }, frame);
}

async function installClientSpy(page: Page): Promise<void> {
  await page.waitForFunction(
    () =>
      typeof (window as unknown as { __wanloggerSetClient?: unknown })
        .__wanloggerSetClient === "function",
  );
  await page.evaluate(() => {
    const sent: unknown[] = [];
    const win = window as unknown as {
      __wanloggerSetClient: (client: { send: (frame: unknown) => void }) => void;
      __wanloggerSentFrames: unknown[];
    };
    win.__wanloggerSentFrames = sent;
    win.__wanloggerSetClient({ send: (frame: unknown) => sent.push(frame) });
  });
}

async function sentFrames(page: Page): Promise<unknown[]> {
  return page.evaluate(
    () => (window as unknown as { __wanloggerSentFrames?: unknown[] }).__wanloggerSentFrames ?? [],
  );
}

test("loads shell and shows top-bar title", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("wanlogger").first()).toBeVisible();
  await expect(page.getByText(/Terminal|\u30bf\u30fc\u30df\u30ca\u30eb/).first()).toBeVisible();
});

test("language toggle switches between ja and en", async ({ page }) => {
  await page.goto("/");
  const toggle = page.getByRole("button", { name: /JA|EN/ });
  const before = await toggle.textContent();
  await toggle.click();
  const after = await toggle.textContent();
  expect(after).not.toEqual(before);
});

test("injected ctl error frame surfaces a toast", async ({ page }) => {
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
      type: "ctl",
      seq: 1,
      payload: {
        event: "auth_failed",
        message: "bad token in e2e",
        error_id: "E-2001",
      },
  });
  await expect(page.getByText("bad token in e2e")).toBeVisible();
  await expect(page.getByText("E-2001")).toBeVisible();
});

test("injected data frame populates the sources panel", async ({ page }) => {
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
      type: "data",
      seq: 1,
      payload: {
        ts_origin: 0,
        ts_ingest: 1_000_000,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "e2e-source",
        ch: 0,
        dir: "in",
        kind: "bytes",
        body: new Uint8Array([72, 73]),
        source: "uart-e2e",
      },
  });
  await expect(page.getByRole("cell", { name: "uart-e2e" })).toBeVisible();
});

test("source alias updates terminal and tile labels", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "data",
    seq: 1,
    payload: {
      ts_origin: 0,
      ts_ingest: 1_000_000,
      mono_ns: 0,
      boot_id: "b",
      node_id: "n",
      clock_offset_ms: 0,
      clock_quality: "best-effort",
      drift_ppm: 0,
      clock_source: "system",
      sid: "alias-source",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "serial:COM7",
    },
  });

  await expect(page.getByText(/COM7 \/ ch 0/).first()).toBeVisible();
  await page.getByRole("button", { name: /Details|�ڍ�/ }).first().click();
  await page.getByLabel(/Display alias|�\�����G�C���A�X/).fill("Motor UART");

  await expect(page.getByText(/Motor UART \/ ch 0/).first()).toBeVisible();
  await expect(page.locator(".wl-tile-header").filter({ hasText: "Motor UART" })).toBeVisible();
});

test("tile xterm viewport remains mouse-scrollable", async ({ page }) => {
  // REQ: FR-UI-012
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "data",
    seq: 1,
    payload: {
      ts_origin: 0,
      ts_ingest: 1_000_000,
      mono_ns: 0,
      boot_id: "b",
      node_id: "n",
      clock_offset_ms: 0,
      clock_quality: "best-effort",
      drift_ppm: 0,
      clock_source: "system",
      sid: "tile-scroll-source",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73, 10]),
      source: "serial:COM8",
    },
  });

  const viewport = page.locator(".wl-tile-body .xterm-viewport").first();
  await expect(viewport).toBeVisible();

  const styles = await viewport.evaluate((node) => {
    const viewportStyle = window.getComputedStyle(node);
    const bodyStyle = window.getComputedStyle(node.closest(".wl-tile-body") as Element);
    return {
      viewportOverflowY: viewportStyle.overflowY,
      pointerEvents: viewportStyle.pointerEvents,
      bodyOverflowY: bodyStyle.overflowY,
    };
  });

  expect(styles.viewportOverflowY).not.toBe("hidden");
  expect(styles.bodyOverflowY).toBe("hidden");
  expect(styles.pointerEvents).not.toBe("none");
});

test("tile viewport auto-follows the newest log while at bottom", async ({ page }) => {
  // REQ: FR-UI-012
  await page.goto("/");
  await waitForInject(page);

  for (let seq = 0; seq < 80; seq += 1) {
    await injectFrame(page, {
      type: "data",
      seq,
      payload: {
        ts_origin: seq,
        ts_ingest: seq + 1,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "tile-follow-source",
        ch: 0,
        dir: "in",
        kind: "bytes",
        body: new TextEncoder().encode(`line-${seq}\n`),
        source: "serial:COM9",
      },
    });
  }

  const viewport = page.locator(".wl-tile-body .xterm-viewport").first();
  await expect(viewport).toBeVisible();
  await expect.poll(async () => viewport.evaluate((node) => (
    node.scrollHeight - node.scrollTop - node.clientHeight
  ))).toBeLessThan(8);
});

test("tile viewport preserves manual scroll and resumes bottom follow", async ({ page }) => {
  // REQ: FR-UI-012
  await page.goto("/");
  await waitForInject(page);

  for (let seq = 0; seq < 90; seq += 1) {
    await injectFrame(page, {
      type: "data",
      seq,
      payload: {
        ts_origin: seq,
        ts_ingest: seq + 1,
        mono_ns: 0,
        boot_id: "b",
        node_id: "n",
        clock_offset_ms: 0,
        clock_quality: "best-effort",
        drift_ppm: 0,
        clock_source: "system",
        sid: "tile-manual-scroll-source",
        ch: 0,
        dir: "in",
        kind: "bytes",
        body: new TextEncoder().encode(`before-${seq}\n`),
        source: "serial:COM10",
      },
    });
  }

  const viewport = page.locator(".wl-tile-body .xterm-viewport").first();
  await expect(viewport).toBeVisible();
  await expect.poll(async () => viewport.evaluate((node) => (
    node.scrollHeight - node.scrollTop - node.clientHeight
  ))).toBeLessThan(8);

  await viewport.evaluate((node) => {
    node.scrollTop = 0;
    node.dispatchEvent(new Event("scroll"));
  });

  await injectFrame(page, {
    type: "data",
    seq: 91,
    payload: {
      ts_origin: 91,
      ts_ingest: 92,
      mono_ns: 0,
      boot_id: "b",
      node_id: "n",
      clock_offset_ms: 0,
      clock_quality: "best-effort",
      drift_ppm: 0,
      clock_source: "system",
      sid: "tile-manual-scroll-source",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new TextEncoder().encode("while-reading-old-lines\n"),
      source: "serial:COM10",
    },
  });

  await expect.poll(async () => viewport.evaluate((node) => (
    node.scrollHeight - node.scrollTop - node.clientHeight
  ))).toBeGreaterThan(16);

  await viewport.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
    node.dispatchEvent(new Event("scroll"));
  });
  await injectFrame(page, {
    type: "data",
    seq: 92,
    payload: {
      ts_origin: 92,
      ts_ingest: 93,
      mono_ns: 0,
      boot_id: "b",
      node_id: "n",
      clock_offset_ms: 0,
      clock_quality: "best-effort",
      drift_ppm: 0,
      clock_source: "system",
      sid: "tile-manual-scroll-source",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new TextEncoder().encode("follow-again\n"),
      source: "serial:COM10",
    },
  });

  await expect.poll(async () => viewport.evaluate((node) => (
    node.scrollHeight - node.scrollTop - node.clientHeight
  ))).toBeLessThan(8);
});

test("source detail export button calls the HTTP export API", async ({ page }) => {
  // REQ: FR-EXP-001
  let exportUrl = "";
  await page.route("http://127.0.0.1:9000/api/sessions/**/export?**", async (route) => {
    exportUrl = route.request().url();
    await route.fulfill({
      status: 200,
      headers: { "content-type": "text/plain; charset=utf-8" },
      body: "2024-01-01T09:00:00+09:00\tvirt-peer-e2e\n",
    });
  });

  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 2,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "11111111-1111-4111-8111-111111111111",
          name: "Export Source",
          kind: "tcp",
          status: "stopped",
          channels: [0],
          bytes_in: 42,
          persistent: true,
          session_dir: "C:/tmp/wanlogger-session",
        },
      ],
    },
  });

  await expect(page.getByRole("cell", { name: "Export Source" })).toBeVisible();
  await page.getByRole("button", { name: "Details" }).click();
  await page.getByLabel("Export timezone").fill("GMT+9");
  await page.getByLabel("Export filename pattern").fill("{source}_{timestamp}.{ext}");
  await page.getByRole("button", { name: "Download text" }).click();

  await expect.poll(() => exportUrl).toContain("/api/sessions/11111111-1111-4111-8111-111111111111/export");
  expect(exportUrl).toContain("format=text");
  expect(exportUrl).toContain("tz=GMT%2B9");
  await expect(page.getByText("Export download requested")).toBeVisible();
});

test("settings rules and source start defaults are sent with ctl start", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);

  await page.getByLabel("Default text encoding").fill("shift_jis");
  await page.getByLabel("Session name pattern").fill("{prefix}_{kind}_{iface}_{unix_ns}");
  await page.getByPlaceholder("ERROR, WARN, voltage...").fill("ERROR");
  await page.getByPlaceholder("fault, warning, power...").fill("fault");
  await page.getByRole("button", { name: "Add rule" }).click();
  await expect(page.getByRole("cell", { name: "fault", exact: true })).toBeVisible();

  await page.getByLabel("Source spec").fill("mock://phase3-ui");
  await page.getByRole("button", { name: "Add source" }).click();
  await expect(page.getByText("Source start requested")).toBeVisible();

  const frames = await sentFrames(page);
  const start = frames.find((frame) => {
    const candidate = frame as { type?: string; payload?: { action?: string } };
    return candidate.type === "ctl" && candidate.payload?.action === "start";
  }) as {
    payload?: {
      encoding?: string;
      session_name_pattern?: string;
      classifier?: Array<{ contains?: string; tag?: string }>;
    };
  } | undefined;

  expect(start?.payload?.encoding).toBe("shift_jis");
  expect(start?.payload?.session_name_pattern).toBe("{prefix}_{kind}_{iface}_{unix_ns}");
  expect(start?.payload?.classifier).toContainEqual({ contains: "ERROR", tag: "fault" });
});

test("source details expose persistence, per-source display settings, and notes", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 3,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "22222222-2222-4222-8222-222222222222",
          name: "COM7 Logger",
          kind: "serial",
          status: "running",
          channels: [0, 1],
          bytes_in: 128,
          persistent: true,
          session_dir: "C:/logs/COM7-session",
        },
      ],
    },
  });

  await page.getByRole("button", { name: "Details" }).click();
  await expect(page.getByText("Saved to session-dir")).toBeVisible();
  await expect(page.getByText("C:/logs/COM7-session")).toBeVisible();

  await page.getByLabel("Display encoding").fill("cp932");
  await page.getByLabel("Channel encoding ch 1").fill("shift_jis");
  await page.getByLabel("Display alias").fill("Motor COM7");
  await page.getByLabel("Notes").fill("Investigate boot noise");
  await page.getByRole("button", { name: "Restart with encoding" }).click();
  await expect(page.getByText(/Motor COM7 \/ ch 0/).first()).toBeVisible();

  const frames = await sentFrames(page);
  const restart = frames.find((frame) => {
    const candidate = frame as { type?: string; sid?: string; payload?: { action?: string } };
    return candidate.type === "ctl" && candidate.sid === "22222222-2222-4222-8222-222222222222" && candidate.payload?.action === "restart";
  }) as { payload?: { encoding?: string } } | undefined;
  expect(restart?.payload?.encoding).toBe("cp932");
});

test("source notes load from and sync to the annotation API", async ({ page }) => {
  // REQ: FR-UI-017
  const sid = "44444444-4444-4444-8444-444444444444";
  let savedBody: unknown = null;
  await page.route("http://127.0.0.1:9000/api/annotations**", async (route) => {
    const request = route.request();
    if (request.method() === "GET") {
      await route.fulfill({
        status: 200,
        headers: { "content-type": "application/json" },
        body: JSON.stringify([
          {
            id: "55555555-5555-5555-8555-555555555555",
            target: { kind: "session", sid },
            text: "server memo",
            updated_at: "2026-05-20T00:00:00Z",
            deleted: false,
          },
        ]),
      });
      return;
    }
    if (request.method() === "PUT") {
      savedBody = JSON.parse(request.postData() ?? "{}");
      await route.fulfill({
        status: 200,
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          id: "55555555-5555-5555-8555-555555555555",
          ...(savedBody as Record<string, unknown>),
          updated_at: "2026-05-20T00:00:01Z",
          deleted: false,
        }),
      });
      return;
    }
    await route.fulfill({ status: 204 });
  });

  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 4,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "Annotated Source",
          kind: "serial",
          status: "running",
          channels: [0],
          bytes_in: 1,
          persistent: true,
          session_dir: "C:/logs/annotated",
        },
      ],
    },
  });

  await page.getByRole("button", { name: "Details" }).click();
  const details = page.locator("aside");
  await expect(details.getByLabel("Notes")).toHaveValue("server memo");
  await expect(details.getByText("Synced", { exact: true })).toBeVisible();

  await details.getByLabel("Notes").fill("client memo");
  await details.getByRole("button", { name: "Sync now" }).click();
  await expect.poll(() => savedBody).toMatchObject({
    target: { kind: "session", sid },
    text: "client memo",
  });
  await expect(details.getByText("Synced", { exact: true })).toBeVisible();
});

test("source note annotation sync failure is visible but non-fatal", async ({ page }) => {
  // REQ: FR-UI-017
  await page.route("http://127.0.0.1:9000/api/annotations**", async (route) => {
    await route.fulfill({ status: 500, body: "annotation store down" });
  });

  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 5,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "66666666-6666-4666-8666-666666666666",
          name: "Failing Annotation Source",
          kind: "serial",
          status: "running",
          channels: [0],
          bytes_in: 1,
          persistent: true,
          session_dir: "C:/logs/failing-annotation",
        },
      ],
    },
  });

  await page.getByRole("button", { name: "Details" }).click();
  const details = page.locator("aside");
  await expect(details.getByText("Sync failed")).toBeVisible();
  await expect(page.getByText("Source note sync failed; kept in this browser.")).toBeVisible();
});
