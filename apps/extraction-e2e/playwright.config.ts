import { defineConfig } from "@playwright/test";

import { loadLiveE2eConfig } from "./src/live/config";

const live = loadLiveE2eConfig();

export default defineConfig({
  fullyParallel: false,
  forbidOnly: true,
  outputDir: live.artifactDir,
  reporter: [["list"]],
  retries: 0,
  testDir: "./src/live",
  testMatch: "**/*.live.spec.ts",
  timeout: live.scenarioTimeoutMs * 3 + 30_000,
  use: {
    baseURL: live.clientUrl,
    channel: process.env.EXTRACTION_E2E_BROWSER_CHANNEL ?? "chrome",
    headless: process.env.EXTRACTION_E2E_HEADED !== "true",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    video: "off",
    viewport: { height: 900, width: 1440 },
  },
  workers: 1,
});
