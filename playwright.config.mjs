import { defineConfig } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const directory = mkdtempSync(join(tmpdir(), "gfa-browser-"));

export default defineConfig({
  testDir: "./tests/browser",
  workers: 1,
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    channel: process.env.CI ? "chrome" : undefined,
    baseURL: "http://127.0.0.1:18080",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [{
    command: `target/debug/gfa serve --sqlite "${join(directory, "matches.sqlite")}" --port 18080`,
    url: "http://127.0.0.1:18080/healthz",
    timeout: 30000,
    reuseExistingServer: false,
    gracefulShutdown: { signal: "SIGTERM", timeout: 10000 },
  }, {
    command: "pnpm --filter @gfa/site dev --port 18081",
    url: "http://127.0.0.1:18081",
    env: { GFA_API_TARGET: "http://127.0.0.1:18080" },
    timeout: 30000,
    reuseExistingServer: false,
    gracefulShutdown: { signal: "SIGTERM", timeout: 10000 },
  }],
});
