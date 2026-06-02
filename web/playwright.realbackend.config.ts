import { defineConfig, devices } from "@playwright/test";

// Dedicated config for the GUI real-backend smoke suite. Unlike the default
// `playwright.config.ts` (injection-based, no server), this spawns a real
// `tracemux serve` + `tracemux-virt-peer` via global setup and drives the live
// UI against them. Run it with `just gui-smoke` (which builds the binaries
// first). Kept separate so the fast injection suite stays driver-free.

export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: /real-backend\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  globalSetup: "./realBackend.global-setup.ts",
  globalTeardown: "./realBackend.global-teardown.ts",
  use: {
    baseURL: "http://127.0.0.1:5173",
    trace: "on-first-retry",
    headless: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "corepack pnpm dev",
    url: "http://127.0.0.1:5173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
