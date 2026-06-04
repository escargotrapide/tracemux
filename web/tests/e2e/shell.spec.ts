// E2E shell test driving the UI through the dev-only
// `window.__tracemuxInject` hook (no real WSS server needed).
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
      typeof (window as unknown as { __tracemuxInject?: unknown })
        .__tracemuxInject === "function",
  );
}

async function injectFrame(page: Page, frame: unknown): Promise<void> {
  await page.evaluate((payload) => {
    const fn = (
      window as unknown as { __tracemuxInject: (f: unknown) => void }
    ).__tracemuxInject;
    fn(payload);
  }, frame);
}

async function installClientSpy(page: Page, sendResult = true): Promise<void> {
  await page.waitForFunction(
    () =>
      typeof (window as unknown as { __tracemuxSetClient?: unknown })
        .__tracemuxSetClient === "function",
  );
  await page.evaluate((result) => {
    const sent: unknown[] = [];
    const win = window as unknown as {
      __tracemuxSetClient: (client: { send: (frame: unknown) => boolean }) => void;
      __tracemuxSentFrames: unknown[];
    };
    win.__tracemuxSentFrames = sent;
    win.__tracemuxSetClient({
      send: (frame: unknown) => {
        sent.push(frame);
        return result;
      },
    });
  }, sendResult);
}

async function setConnState(page: Page, state: unknown): Promise<void> {
  await page.waitForFunction(
    () =>
      typeof (window as unknown as { __tracemuxSetConnState?: unknown })
        .__tracemuxSetConnState === "function",
  );
  await page.evaluate((next) => {
    const fn = (
      window as unknown as { __tracemuxSetConnState: (s: unknown) => void }
    ).__tracemuxSetConnState;
    fn(next);
  }, state);
}

async function sentFrames(page: Page): Promise<unknown[]> {
  return page.evaluate(
    () => (window as unknown as { __tracemuxSentFrames?: unknown[] }).__tracemuxSentFrames ?? [],
  );
}

async function clearSentFrames(page: Page): Promise<void> {
  await page.evaluate(() => {
    const frames = (window as unknown as { __tracemuxSentFrames?: unknown[] })
      .__tracemuxSentFrames;
    if (frames) frames.length = 0;
  });
}

test("loads shell and shows top-bar title", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("tracemux").first()).toBeVisible();
  await expect(page.getByText(/Terminal|\u30bf\u30fc\u30df\u30ca\u30eb/).first()).toBeVisible();
  await expect(page.getByText("Log type note sync failed; saved locally only and not on the server yet.")).toHaveCount(0);
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

  await page.getByTestId("notification-button").click();
  const center = page.getByTestId("notification-center");
  await expect(center.getByText("bad token in e2e")).toBeVisible();
  await expect(center.getByText("E-2001")).toBeVisible();
});

test("error without a runbook still shows an inline remedy", async ({ page }) => {
  // REQ: FR-UI-009
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 1,
    payload: {
      event: "error",
      message: "mystery failure in e2e",
      error_id: "E-9999",
    },
  });

  const toast = page.locator(".wl-toast", { hasText: "mystery failure in e2e" });
  await expect(toast).toBeVisible();
  await expect(toast.getByText("E-9999")).toBeVisible();
  // No public runbook link for this id ...
  await expect(toast.locator(".wl-error-runbook")).toHaveCount(0);
  // ... but a short inline remedy is shown instead.
  await expect(toast.locator(".wl-error-remedy")).toBeVisible();
});

test("dock tabs expose distinct panel accent bands", async ({ page }) => {
  await page.goto("/");

  const accents = await page.locator(".wl-dock-tab").evaluateAll((nodes) => (
    nodes.map((node) => getComputedStyle(node).getPropertyValue("--wl-panel-accent").trim())
      .filter(Boolean)
  ));

  expect(accents.length).toBeGreaterThanOrEqual(6);
  expect(new Set(accents).size).toBeGreaterThanOrEqual(5);
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

test("removing a source asks for confirmation before sending the ctl", async ({ page }) => {
  // REQ: FR-UI-005
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);
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
      sid: "remove-e2e",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "uart-remove",
    },
  });

  const removeButton = page
    .getByRole("row", { name: /uart-remove/ })
    .getByRole("button", { name: /^Remove$|^削除$/ });

  const removeSent = async (): Promise<boolean> =>
    (await sentFrames(page)).some((frame) => {
      const f = frame as { type?: string; sid?: string; payload?: { action?: string } };
      return f.type === "ctl" && f.sid === "remove-e2e" && f.payload?.action === "remove";
    });

  // Dismissing the confirmation must not send a remove ctl.
  page.once("dialog", (dialog) => {
    expect(dialog.message()).toMatch(/Remove this source|サーバーの登録一覧から削除/);
    void dialog.dismiss();
  });
  await removeButton.click();
  expect(await removeSent()).toBe(false);

  // Accepting the confirmation sends the remove ctl.
  page.once("dialog", (dialog) => void dialog.accept());
  await removeButton.click();
  await expect.poll(removeSent).toBe(true);
});

test("export filename pattern shows a length counter and enforces the limit", async ({ page }) => {
  // REQ: FR-UI-008
  await page.goto("/");
  await waitForInject(page);

  const section = page.locator(".wl-source-bulk-export");
  const input = section.getByLabel(/Shared export filename pattern|共通エクスポートファイル名パターン/);
  const counter = section.locator(".wl-source-count");

  await input.fill("tracemux-{source}.{ext}");
  await expect(counter).toHaveText(/^\d+\/240$/);
  await expect(counter).not.toHaveClass(/wl-source-count-limit/);

  // The limit is surfaced rather than silently truncating without feedback.
  await input.fill("x".repeat(300));
  await expect(input).toHaveValue("x".repeat(240));
  await expect(counter).toHaveText("240/240");
  await expect(counter).toHaveClass(/wl-source-count-limit/);
});


test("source details distinguish server-side encoding from the browser display override", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);

  await injectFrame(page, {
    type: "ctl",
    seq: 70,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "77777777-7777-4777-8777-777777777777",
          name: "enc-source",
          kind: "serial",
          status: "running",
          channels: [0],
          bytes_in: 0,
          encoding: "shift_jis",
        },
      ],
    },
  });

  const row = page.getByRole("row").filter({ hasText: "enc-source" });
  await expect(row.getByRole("cell", { name: "enc-source" })).toBeVisible();
  await row.getByRole("button", { name: /^Details$|^詳細$/ }).click();

  const aside = page.locator("aside");
  // The server-side decoded/persisted encoding is shown distinctly.
  const serverLine = aside.locator(".wl-encoding-server");
  await expect(serverLine).toContainText(/Server-side encoding|サーバー側エンコーディング/);
  await expect(serverLine.locator("code")).toHaveText("shift_jis");

  // With no browser override, the effective display value is shown as inherited.
  const sourceSelect = aside.getByLabel("Display encoding");
  const originBadge = sourceSelect.locator("xpath=following-sibling::span[1]");
  await expect(originBadge).toHaveText(/Inherited from server|サーバーから継承/);

  // Choosing a different display encoding flips the origin to a source override.
  await sourceSelect.selectOption("euc-jp");
  await expect(originBadge).toHaveText(/Source override|ソース上書き/);
  // The server-side value is unchanged by a browser-only display override.
  await expect(serverLine.locator("code")).toHaveText("shift_jis");
});


test("manual source start shows a pending acknowledgement until the server registers it", async ({ page }) => {
  // REQ: FR-UI-008
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);

  await page.getByLabel("Source spec").fill("mock://pending-e2e");
  await page.getByRole("button", { name: "Add source" }).click();

  // The request is reflected as a pending state, not just a transient toast.
  const pending = page.locator(".wl-source-pending");
  await expect(pending).toBeVisible();
  await expect(pending.getByText("Waiting for server acknowledgement:")).toBeVisible();
  await expect(pending.getByText("mock://pending-e2e")).toBeVisible();

  // When the server acknowledges by registering a source, the pending entry clears.
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
      sid: "pending-ack-e2e",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "mock-pending",
    },
  });
  await expect(page.getByRole("cell", { name: "mock-pending" })).toBeVisible();
  await expect(pending.getByText("mock://pending-e2e")).toHaveCount(0);
});


test("source notes show a local-only fallback notice when server sync fails", async ({ page }) => {
  // REQ: FR-UI-008
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);
  // The note load effect only runs while the WSS reports an open connection.
  await setConnState(page, { status: "open", detail: "" });

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
      sid: "notes-fallback-e2e",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "uart-notes",
    },
  });

  const row = page.getByRole("row").filter({ hasText: "uart-notes" });
  await expect(row.getByRole("cell", { name: "uart-notes" })).toBeVisible();
  await row.getByRole("button", { name: /^Details$|^詳細$/ }).click();

  // With no reachable server the annotation fetch fails, so the note is kept
  // in the browser only and the UI must say so explicitly.
  await expect(
    page.getByText(
      /this note is kept in this browser only|このメモはこのブラウザ内だけに保存され/,
    ),
  ).toBeVisible();
});


test("multi-channel source lets the user pick which channel opens", async ({ page }) => {
  // REQ: FR-UI-008
  const sid = "44444444-4444-4444-8444-444444444444";
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 44,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "multi-ch",
          kind: "tcp",
          status: "running",
          channels: [0, 1, 2],
          bytes_in: 0,
        },
      ],
    },
  });

  const row = page.getByRole("row").filter({ hasText: "multi-ch" });
  await expect(row.getByRole("cell", { name: "multi-ch" })).toBeVisible();

  // A channel selector appears only because the source exposes >1 channel.
  const channelSelect = row.getByLabel(/Channel to open|開くチャンネル/);
  await expect(channelSelect).toBeVisible();
  await expect(channelSelect.getByRole("option")).toHaveCount(3);

  // The open buttons default to the first channel.
  await expect(row.getByRole("button", { name: /Open terminal ch 0|端末で開く ch 0/ })).toBeVisible();

  // Choosing channel 2 updates both open actions to target it.
  await channelSelect.selectOption("2");
  await expect(
    row.getByRole("button", { name: /Open terminal ch 2|端末で開く ch 2/ }),
  ).toBeVisible();
  await expect(
    row.getByRole("button", { name: /New terminal ch 2|新規端末 ch 2/ }),
  ).toBeVisible();

  // Opening the terminal routes the chosen channel; the confirmation toast
  // reflects the selected channel rather than the default first channel.
  await row.getByRole("button", { name: /Open terminal ch 2|端末で開く ch 2/ }).click();
  await expect(page.getByText(/\(ch 2\)/)).toBeVisible();
});

test("packet capture panel shows bounded buffer stats", async ({ page }) => {
  // REQ: FR-UI-PCAP
  // REQ: NFR-PERF-PCAP
  const sid = "77777777-7777-4777-8777-777777777777";
  await page.goto("/");
  await waitForInject(page);
  const packetPanel = page.locator('div.wl-panel-content[data-panel-kind="packet"]').first();
  await injectFrame(page, {
    type: "ctl",
    seq: 77,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "pcap0",
          kind: "pcap",
          status: "running",
          channels: [0],
          bytes_in: 0,
          persistent: true,
          session_dir: "C:/logs/pcap0",
        },
      ],
    },
  });

  await expect(packetPanel.getByLabel("Source")).toHaveValue(sid);
  await expect(packetPanel.getByText("Buffer: 512")).toBeVisible();
  await expect(packetPanel.getByText("Dropped: 0")).toBeVisible();

  await injectFrame(page, {
    type: "data",
    seq: 78,
    payload: {
      ts_origin: 1_700_000_000_123_456_789,
      ts_ingest: 1_700_000_000_223_456_789,
      mono_ns: 42,
      boot_id: "b",
      node_id: "n",
      clock_offset_ms: 0,
      clock_quality: "best-effort",
      drift_ppm: 0,
      clock_source: "system",
      sid,
      ch: 0,
      dir: "in",
      kind: "datagram",
      body: new Uint8Array([
        0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
        0x08, 0x00,
      ]),
      source: "pcap:pcap0",
    },
  });

  await expect(packetPanel.getByText("Packets: 1")).toBeVisible();
  await expect(packetPanel.getByRole("cell", { name: "ipv4" })).toBeVisible();
});

test("packet capture panel explains publish modes and the stats-only empty state", async ({ page }) => {
  // REQ: FR-UI-PCAP
  const sid = "88888888-8888-4888-8888-888888888888";
  await page.goto("/");
  await waitForInject(page);
  const packetPanel = page.locator('div.wl-panel-content[data-panel-kind="packet"]').first();
  await injectFrame(page, {
    type: "ctl",
    seq: 88,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "pcap-quiet",
          kind: "pcap",
          status: "running",
          channels: [0],
          bytes_in: 0,
          persistent: true,
          session_dir: "C:/logs/pcap-quiet",
        },
      ],
    },
  });

  await expect(packetPanel.getByLabel("Source")).toHaveValue(sid);
  // Publish-mode legend is always available.
  await expect(packetPanel.getByText(/Publish modes|配信モード/)).toBeVisible();
  // With no packets yet, the stats-only hint clarifies the empty list is by design.
  await expect(
    packetPanel.locator(".wl-packet-empty-hint"),
  ).toBeVisible();
  await expect(packetPanel.locator(".wl-packet-empty-hint")).toContainText(
    /stays empty by design|仕様上空のまま/,
  );
});

test("packet capture panel warns when the UI ring drops packets", async ({ page }) => {
  // REQ: FR-UI-PCAP
  // REQ: NFR-PERF-PCAP
  const sid = "99999999-9999-4999-8999-999999999999";
  await page.goto("/");
  await waitForInject(page);
  const packetPanel = page.locator('div.wl-panel-content[data-panel-kind="packet"]').first();
  await injectFrame(page, {
    type: "ctl",
    seq: 99,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "pcap-flood",
          kind: "pcap",
          status: "running",
          channels: [0],
          bytes_in: 0,
          persistent: true,
          session_dir: "C:/logs/pcap-flood",
        },
      ],
    },
  });
  await expect(packetPanel.getByLabel("Source")).toHaveValue(sid);

  // Overflow the bounded ring (capacity 512) to trigger drops.
  await page.evaluate((targetSid) => {
    const inject = (window as unknown as {
      __tracemuxInject: (frame: unknown) => void;
    }).__tracemuxInject;
    for (let i = 0; i < 600; i += 1) {
      inject({
        type: "data",
        seq: 1000 + i,
        payload: {
          ts_origin: 1_700_000_000_000_000_000 + i,
          ts_ingest: 1_700_000_000_000_000_000 + i,
          mono_ns: i,
          boot_id: "b",
          node_id: "n",
          clock_offset_ms: 0,
          clock_quality: "best-effort",
          drift_ppm: 0,
          clock_source: "system",
          sid: targetSid,
          ch: 0,
          dir: "in",
          kind: "datagram",
          body: new Uint8Array([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00]),
          source: "pcap:pcap-flood",
        },
      });
    }
  }, sid);

  await expect(
    packetPanel.getByText(/dropped from the UI buffer|UIバッファから破棄/),
  ).toBeVisible();
});

test("pcap interface selector surfaces flags and addresses", async ({ page }) => {
  // REQ: FR-UI-PCAP
  await page.route("**/api/detect", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        serial_candidates: [],
        pcap_interfaces: [
          {
            device: "eth0",
            display_name: "Ethernet",
            description: "Primary NIC",
            addresses: ["192.168.1.10", "fe80::1"],
            flags: ["UP", "RUNNING"],
          },
          {
            device: "eth1",
            display_name: "Down NIC",
            description: "",
            addresses: [],
            flags: ["DOWN"],
          },
        ],
      }),
    });
  });

  await page.goto("/");
  await waitForInject(page);
  const sourcesPanel = page
    .locator('div.wl-panel-content[data-panel-kind="sources"]')
    .first();
  await sourcesPanel
    .getByRole("button", { name: /Detect COM ports|COMポート検出/ })
    .click();

  const ifaceInfo = sourcesPanel.locator(".wl-pcap-iface-info");
  await expect(ifaceInfo).toBeVisible();
  // Select the up interface explicitly and verify its flags/addresses.
  await sourcesPanel.getByLabel(/Interface|インターフェース/).selectOption("eth0");
  await expect(ifaceInfo).toContainText(/Flags|フラグ/);
  await expect(ifaceInfo).toContainText("RUNNING, UP");
  await expect(ifaceInfo).toContainText(/Addresses|アドレス/);
  await expect(ifaceInfo).toContainText("192.168.1.10, fe80::1");

  // Selecting the down interface warns the user.
  await sourcesPanel.getByLabel(/Interface|インターフェース/).selectOption("eth1");
  await expect(sourcesPanel.locator(".wl-pcap-iface-down")).toBeVisible();
  await expect(sourcesPanel.locator(".wl-pcap-iface-empty")).toBeVisible();
});

test("terminal toolbar changes the selected channel text encoding", async ({ page }) => {
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
      sid: "33333333-3333-4333-8333-333333333333",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([0x82, 0xa0]),
      source: "serial:COM11",
    },
  });

  const terminal = page.locator('div.wl-panel-content[data-panel-kind="terminal"]').first();
  await expect(terminal.getByText(/COM11 \/ ch 0/)).toBeVisible();
  await expect(terminal.getByLabel(/Text encoding|文字コード/)).toBeEnabled();
  await terminal.getByLabel(/Text encoding|文字コード/).selectOption("shift_jis");
  await expect.poll(() => page.evaluate(async () => {
    const { sourceEncodings } = await import("/src/state/sourceEncodings.ts");
    return sourceEncodings["33333333-3333-4333-8333-333333333333/0"]?.encoding ?? "";
  })).toBe("shift_jis");

  await expect.poll(() => page.evaluate(async () => {
    const { getChannelFrames } = await import("/src/state/channelBuffers.ts");
    const { bodyText } = await import("/src/state/displayFrames.ts");
    const { encodingForChannel } = await import("/src/state/sourceEncodings.ts");
    const sid = "33333333-3333-4333-8333-333333333333";
    const frame = getChannelFrames(sid, 0, 1)[0];
    return frame ? bodyText(frame, encodingForChannel(sid, 0, "utf-8")) : "";
  })).toBe("あ");
  await expect(page.locator(".wl-tile").filter({ hasText: "COM11" })).toHaveAttribute(
    "data-encoding",
    "shift_jis",
  );
});

test("changing terminal encoding on a large buffer asks for confirmation", async ({ page }) => {
  // REQ: FR-UI-014
  const sid = "88888888-8888-4888-8888-888888888888";
  await page.goto("/");
  await waitForInject(page);

  // Fill the channel past the redraw-confirmation threshold.
  await page.evaluate((targetSid) => {
    const inject = (
      window as unknown as { __tracemuxInject: (f: unknown) => void }
    ).__tracemuxInject;
    for (let i = 0; i < 600; i += 1) {
      inject({
        type: "data",
        seq: i + 1,
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
          sid: targetSid,
          ch: 0,
          dir: "in",
          kind: "bytes",
          body: new Uint8Array([0x41]),
          source: "serial:COM41",
        },
      });
    }
  }, sid);

  const terminal = page.locator('div.wl-panel-content[data-panel-kind="terminal"]').first();
  await expect(terminal.getByText(/COM41 \/ ch 0/)).toBeVisible();
  const encodingSelect = terminal.getByLabel(/Text encoding|文字コード/);

  // Dismissing the confirmation leaves the encoding unchanged.
  page.once("dialog", (dialog) => {
    expect(dialog.message()).toMatch(/Change the display encoding|表示エンコーディングを変更/);
    void dialog.dismiss();
  });
  await encodingSelect.selectOption("shift_jis");
  await expect.poll(() => page.evaluate(async (targetSid) => {
    const { sourceEncodings } = await import("/src/state/sourceEncodings.ts");
    return sourceEncodings[`${targetSid}/0`]?.encoding ?? "";
  }, sid)).toBe("");

  // Accepting the confirmation applies the new encoding.
  page.once("dialog", (dialog) => void dialog.accept());
  await encodingSelect.selectOption("shift_jis");
  await expect.poll(() => page.evaluate(async (targetSid) => {
    const { sourceEncodings } = await import("/src/state/sourceEncodings.ts");
    return sourceEncodings[`${targetSid}/0`]?.encoding ?? "";
  }, sid)).toBe("shift_jis");
});

test("terminal send controls disable and explain why when the WSS is closed", async ({ page }) => {
  // REQ: FR-UI-009
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
      sid: "55555555-5555-4555-8555-555555555555",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "serial:COM21",
    },
  });

  const terminal = page.locator('div.wl-panel-content[data-panel-kind="terminal"]').first();
  await expect(terminal.getByText(/COM21 \/ ch 0/)).toBeVisible();

  const sendInput = terminal.locator(".wl-terminal-send-input");
  const sendButton = terminal.getByRole("button", { name: /Send$|送信$/ });

  // With a source selected and the connection open, controls are usable.
  await setConnState(page, { status: "open", since: Date.now() });
  await expect(sendInput).toBeEnabled();
  await sendInput.fill("ping");
  await expect(sendButton).toBeEnabled();

  // Losing the connection disables both the input and the button and
  // explains, via the control title, that commands cannot be sent.
  await setConnState(page, { status: "closed", code: 1006, reason: "lost" });
  await expect(sendInput).toBeDisabled();
  await expect(sendButton).toBeDisabled();
  await expect(sendInput).toHaveAttribute(
    "title",
    /Commands will not be sent until the connection reopens|再接続するまでコマンドは送信されません/,
  );

  // Reconnecting restores the controls.
  await setConnState(page, { status: "open", since: Date.now() });
  await expect(sendInput).toBeEnabled();
  await expect(sendButton).toBeEnabled();
});

test("tile grid marks data as stale when the WSS is not open", async ({ page }) => {
  // REQ: FR-UI-009
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
      sid: "66666666-6666-4666-8666-666666666666",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73]),
      source: "serial:COM31",
    },
  });

  const grid = page.getByTestId("tile-grid");
  await expect(grid).toBeVisible();

  // Live: no stale marker on the grid and no stale note in the toolbar.
  await setConnState(page, { status: "open", since: Date.now() });
  await expect(grid).not.toHaveAttribute("data-stale", "true");
  await expect(page.locator(".wl-tile-stale")).toHaveCount(0);

  // Disconnected: the grid is flagged stale and the toolbar warns that the
  // displayed data is the last received, not live.
  await setConnState(page, { status: "closed", code: 1006, reason: "lost" });
  await expect(grid).toHaveAttribute("data-stale", "true");
  await expect(
    page.locator(".wl-tile-stale").getByText(/showing last received data|最後に受信したデータ/),
  ).toBeVisible();

  // Reconnecting clears the stale state.
  await setConnState(page, { status: "open", since: Date.now() });
  await expect(grid).not.toHaveAttribute("data-stale", "true");
  await expect(page.locator(".wl-tile-stale")).toHaveCount(0);
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
  await page.getByRole("button", { name: /Details|詳細/ }).first().click();
  await page.getByLabel(/Display alias|表示名エイリアス/).fill("Motor UART");

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

test("tile grid offers a rendering pause escape hatch", async ({ page }) => {
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
      sid: "tile-pause-source",
      ch: 0,
      dir: "in",
      kind: "bytes",
      body: new Uint8Array([72, 73, 10]),
      source: "serial:COM12",
    },
  });

  const grid = page.getByTestId("tile-grid");
  await expect(grid).toBeVisible();
  await expect(grid).not.toHaveAttribute("data-paused", "true");

  const pauseButton = page.getByRole("button", { name: /Pause rendering|描画を一時停止$/ });
  await pauseButton.click();

  await expect(grid).toHaveAttribute("data-paused", "true");
  await expect(page.getByText(/Rendering paused|描画を一時停止中/)).toBeVisible();

  const resumeButton = page.getByRole("button", { name: /Resume rendering|描画を再開/ });
  await expect(resumeButton).toBeVisible();
  await resumeButton.click();

  await expect(grid).not.toHaveAttribute("data-paused", "true");
  await expect(page.getByText(/Rendering paused|描画を一時停止中/)).toBeHidden();
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
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "content-disposition": "attachment; filename=export-source.txt",
      },
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
          session_dir: "C:/tmp/tracemux-session",
        },
      ],
    },
  });

  await expect(page.getByRole("cell", { name: "Export Source" })).toBeVisible();
  await page.getByRole("button", { name: "Details" }).click();
  await page.getByLabel("Shared export timezone").last().fill("GMT+9");
  await page.getByLabel("Shared export filename pattern").last().fill("{source}_{timestamp}.{ext}");
  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download text" }).click();
  const download = await downloadPromise;

  await expect.poll(() => exportUrl).toContain("/api/sessions/11111111-1111-4111-8111-111111111111/export");
  expect(exportUrl).toContain("format=text");
  expect(exportUrl).toContain("tz=GMT%2B9");
  expect(await download.failure()).toBeNull();
  await expect(page.getByText("Export download requested")).toBeVisible();
});

test("source detail shows recent source lifecycle errors", async ({ page }) => {
  // REQ: FR-UI-009
  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 18,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "99999999-9999-4999-8999-999999999999",
          name: "Faulty Source",
          kind: "serial",
          status: "stopped",
          channels: [0],
          bytes_in: 0,
          persistent: false,
        },
      ],
    },
  });
  await injectFrame(page, {
    type: "ctl",
    seq: 19,
    payload: {
      event: "error",
      sid: "99999999-9999-4999-8999-999999999999",
      message: "source restart failed: access denied",
      error_id: "E-1104",
    },
  });

  await expect(page.getByRole("cell", { name: "Faulty Source" })).toBeVisible();
  await page.getByRole("button", { name: "Details" }).click();
  await expect(page.getByText("Recent errors")).toBeVisible();
  const errorHistory = page.locator(".wl-source-error-history");
  await expect(errorHistory).toContainText("source restart failed: access denied");
  await expect(errorHistory).toContainText("E-1104");
});

test("source panel can bulk export all persisted sources as one zip", async ({ page }) => {
  // REQ: FR-UI-018
  let bundleTicketBody: unknown;
  let bundleDownloadUrl = "";
  await page.route("http://127.0.0.1:9000/api/exports/bundle-ticket", async (route) => {
    bundleTicketBody = route.request().postDataJSON();
    await route.fulfill({
      status: 200,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        ticket: "bundle-ticket",
        expires_in_ms: 60_000,
        expires_at_ms: 1_780_134_200_000,
      }),
    });
  });
  await page.route("http://127.0.0.1:9000/api/exports/bundle?**", async (route) => {
    bundleDownloadUrl = route.request().url();
    await route.fulfill({
      status: 200,
      headers: {
        "content-type": "application/zip",
        "content-disposition": "attachment; filename=tracemux-all.zip",
      },
      body: "PK\x03\x04bundle",
    });
  });

  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 20,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          name: "Bulk A",
          kind: "tcp",
          status: "stopped",
          channels: [0],
          bytes_in: 42,
          persistent: true,
          session_dir: "C:/tmp/bulk-a",
        },
        {
          sid: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          name: "Bulk B",
          kind: "serial",
          status: "stopped",
          channels: [0],
          bytes_in: 64,
          persistent: true,
          session_dir: "C:/tmp/bulk-b",
        },
        {
          sid: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          name: "Live Only",
          kind: "mock",
          status: "running",
          channels: [0],
          bytes_in: 1,
          persistent: false,
        },
      ],
    },
  });

  await page.getByLabel("Shared export timezone").fill("UTC");
  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Zip all text" }).click();
  const download = await downloadPromise;

  await expect.poll(() => bundleTicketBody).toBeTruthy();
  expect(bundleTicketBody).toMatchObject({
    entries: [
      { sid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", source_name: "Bulk A" },
      { sid: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", source_name: "Bulk B" },
    ],
    format: "text",
    tz: "UTC",
  });
  expect(JSON.stringify(bundleTicketBody)).not.toContain("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
  expect(bundleDownloadUrl).toContain("/api/exports/bundle?ticket=bundle-ticket");
  expect(await download.failure()).toBeNull();
  await expect(page.getByText(/Bulk export ZIP download requested/)).toBeVisible();
});

test("source panel can cancel an in-flight bulk export", async ({ page }) => {
  // REQ: FR-UI-018
  let releaseBundleTicket: (() => void) | undefined;
  await page.route("http://127.0.0.1:9000/api/exports/bundle-ticket", async (route) => {
    await new Promise<void>((resolve) => {
      releaseBundleTicket = resolve;
    });
    await route.fulfill({
      status: 200,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        ticket: "cancelled-bundle-ticket",
        expires_in_ms: 60_000,
        expires_at_ms: 1_780_134_200_000,
      }),
    });
  });

  await page.goto("/");
  await waitForInject(page);
  await injectFrame(page, {
    type: "ctl",
    seq: 21,
    payload: {
      event: "sources",
      sources: [
        {
          sid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          name: "Cancellable Bulk",
          kind: "tcp",
          status: "stopped",
          channels: [0],
          bytes_in: 128,
          persistent: true,
          session_dir: "C:/tmp/cancellable-bulk",
        },
      ],
    },
  });

  await page.getByRole("button", { name: "Zip all text" }).click();
  await expect(page.getByText(/Preparing ZIP download TEXT 0\/1/)).toBeVisible();
  await page.getByRole("button", { name: "Cancel export" }).click();
  releaseBundleTicket?.();
  await expect(page.getByText("Bulk export cancelled")).toBeVisible();
  await expect(page.getByRole("button", { name: "Zip all text" })).toBeEnabled();
});

test("settings rules and source start defaults are sent with ctl start", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);

  await page.getByLabel("Time zone").fill("Not/AZone");
  await expect(page.getByText("Invalid time zone; timestamps fall back to local time.")).toBeVisible();
  await page.getByLabel("Default text encoding").selectOption("shift_jis");
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

test("classification rule form flags an invalid regex inline", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);

  await page.getByLabel(/Match type|マッチ種別/).selectOption("regex");
  await page.getByPlaceholder("ERROR, WARN, voltage...").fill("a(");
  await page.getByPlaceholder("fault, warning, power...").fill("regex-tag");

  const error = page.getByText(/Invalid regular expression|正規表現が不正です/);
  await expect(error).toBeVisible();
  await expect(page.getByRole("button", { name: /Add rule|ルール追加/ })).toBeDisabled();

  // Correcting the pattern clears the inline error and re-enables submission.
  await page.getByPlaceholder("ERROR, WARN, voltage...").fill("a(b)");
  await expect(error).toBeHidden();
  await expect(page.getByRole("button", { name: /Add rule|ルール追加/ })).toBeEnabled();
});

test("log type note editing is disabled while sync loads and offers retry on failure", async ({
  page,
}) => {
  // REQ: FR-UI-014
  let failNext = true;
  await page.route("**/api/annotations**", async (route) => {
    if (failNext) {
      failNext = false;
      await route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
      return;
    }
    await route.fulfill({ status: 200, contentType: "application/json", body: "[]" });
  });

  await page.goto("/");
  await waitForInject(page);

  // Bring the connection up so the panel kicks off the annotation load.
  await setConnState(page, { status: "open" });

  const noteField = page.getByPlaceholder(/Free-form memo|自由記述/);
  // The first load fails, exposing the retry path.
  const retry = page.getByRole("button", { name: /Retry sync|同期を再試行/ });
  await expect(retry).toBeVisible();

  // Retrying succeeds, the retry button disappears, and editing is available.
  await retry.click();
  await expect(retry).toBeHidden();
  await expect(noteField).toBeEnabled();
});

test("classification rules explain local-vs-server scope", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);

  const help = page.getByTestId("classification-scope-help");
  await expect(help).toBeVisible();
  await expect(help).toContainText(
    /stored in this browser|このブラウザに保存/,
  );
  await expect(help).toContainText(
    /Send classification rules|分類ルールを送信/,
  );
});

test("numeric display settings show a clamp notice out of range", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);

  const scrollback = page.getByLabel(/Terminal max lines|ターミナル最大行数/);
  await scrollback.fill("1");
  const notice = page
    .locator(".wl-settings-clamp")
    .filter({ hasText: /Adjusted to 100|100 に調整/ });
  await expect(notice.first()).toBeVisible();
  await expect(scrollback).toHaveAttribute("aria-invalid", "true");

  // A value back inside the range clears the notice.
  await scrollback.fill("12000");
  await expect(notice).toHaveCount(0);
  await expect(scrollback).not.toHaveAttribute("aria-invalid", "true");
});

test("log type note shows a live character counter", async ({ page }) => {
  // REQ: FR-UI-014
  await page.goto("/");
  await waitForInject(page);

  const noteField = page.getByPlaceholder(/Free-form memo|自由記述/);
  await noteField.fill("hello");
  await expect(page.getByText("5/20000")).toBeVisible();
});

test("connection banner and unsent source command are visible", async ({ page }) => {
  // REQ: FR-UI-009
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page, false);
  await setConnState(page, { status: "closed", code: 1006, reason: "lost" });

  await expect(page.getByText(/Disconnected from the TraceMux server/)).toBeVisible();
  await page.getByLabel("Source spec").fill("mock://disconnected-e2e");
  await page.getByRole("button", { name: "Add source" }).click();
  await expect(page.getByText(/Request was not sent/)).toBeVisible();
});

test("reconnect replays source list request and active channel subscription", async ({ page }) => {
  // REQ: FR-UI-009
  // REQ: FR-UI-011
  const sid = "99999999-9999-4999-8999-999999999999";
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);

  await injectFrame(page, {
    type: "ctl",
    seq: 9,
    payload: {
      event: "sources",
      sources: [
        {
          sid,
          name: "pcap-reconnect",
          kind: "pcap",
          status: "running",
          channels: [0],
          bytes_in: 0,
          persistent: true,
          session_dir: "C:/logs/pcap-reconnect",
        },
      ],
    },
  });

  await expect
    .poll(async () => {
      const frames = await sentFrames(page);
      return frames.some((frame) => {
        const candidate = frame as { type?: string; sid?: string; ch?: number };
        return candidate.type === "sub" && candidate.sid === sid && candidate.ch === 0;
      });
    })
    .toBe(true);

  await clearSentFrames(page);
  await setConnState(page, { status: "closed", code: 1006, reason: "lost" });
  await setConnState(page, { status: "open", since: Date.now() });

  const frames = await sentFrames(page);
  expect(frames).toContainEqual({ type: "ctl", payload: { action: "list" } });
  expect(frames).toContainEqual({ type: "sub", sid, ch: 0, payload: {} });
});

test("source list stays usable with 1000 live sources", async ({ page }) => {
  // REQ: FR-UI-008
  // REQ: NFR-PERF-001
  const sourceCount = 1000;
  await page.goto("/");
  await waitForInject(page);
  await installClientSpy(page);

  await injectFrame(page, {
    type: "ctl",
    seq: 1000,
    payload: {
      event: "sources",
      sources: Array.from({ length: sourceCount }, (_, sourceIndex) => {
        const suffix = sourceIndex.toString().padStart(4, "0");
        return {
          sid: `00000000-0000-4000-8000-${sourceIndex.toString().padStart(12, "0")}`,
          name: `source-${suffix}`,
          kind: sourceIndex % 10 === 0 ? "pcap" : "mock",
          status: sourceIndex % 5 === 0 ? "stopped" : "running",
          channels: [0],
          bytes_in: sourceIndex * 64,
          persistent: sourceIndex % 3 === 0,
          session_dir: `C:/logs/source-${suffix}`,
        };
      }),
    },
  });

  const sourceRows = page.locator(".wl-sources-table tbody tr");
  await expect(sourceRows).toHaveCount(sourceCount, { timeout: 10_000 });
  await expect(page.getByRole("cell", { name: "source-0999" })).toBeVisible();
  await expect(page.locator(".wl-tile")).toHaveCount(16);

  await expect
    .poll(async () => {
      const frames = await sentFrames(page);
      return new Set(
        frames
          .filter((frame) => {
            const candidate = frame as { type?: string; sid?: string; ch?: number };
            return candidate.type === "sub" && candidate.sid && candidate.ch === 0;
          })
          .map((frame) => (frame as { sid: string; ch: number }).sid),
      ).size;
    })
    .toBe(16);

  await expect
    .poll(() => page.evaluate(async () => {
      const { __flushUiPerfForTest } = await import("/src/state/index.ts");
      return __flushUiPerfForTest().sourceSyncs;
    }))
    .toBeGreaterThanOrEqual(1);

  await page.getByLabel("Search sources").fill("source-0999");
  await expect(sourceRows).toHaveCount(1);
  await expect(page.getByRole("cell", { name: "source-0999" })).toBeVisible();
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

  await expect(page.getByLabel("COM7 Logger Display encoding")).toHaveValue("utf-8");
  await page.getByLabel("COM7 Logger Display encoding").selectOption("cp932");
  await page.getByRole("button", { name: "Details" }).click();
  await expect(page.getByText("Saved to session-dir")).toBeVisible();
  await expect(page.getByText("C:/logs/COM7-session")).toBeVisible();
  await expect(page.getByText("Source note sync failed; saved locally only and not on the server yet.")).toHaveCount(0);
  await expect(page.getByLabel("Display encoding", { exact: true })).toHaveValue("cp932");

  await page.getByLabel("Channel encoding ch 1").selectOption("shift_jis");
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
  await setConnState(page, { status: "open", since: Date.now() });

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
  await setConnState(page, { status: "open", since: Date.now() });

  await page.getByRole("button", { name: "Details" }).click();
  const details = page.locator("aside");
  await expect(details.getByText("Sync failed")).toBeVisible();
  await expect(page.getByText("Source note sync failed; saved locally only and not on the server yet.")).toBeVisible();
});
