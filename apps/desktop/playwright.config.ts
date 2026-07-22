import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e/browser",
  outputDir: "../../test-results/browser",
  reporter: "line",
  use: {
    baseURL: "http://127.0.0.1:15200",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm exec vite --host 127.0.0.1 --port 15200",
    url: "http://127.0.0.1:15200",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
