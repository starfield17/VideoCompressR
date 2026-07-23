import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4444,
  path: "/",
  specs: [resolve(root, "e2e/**/*.e2e.mjs")],
  maxInstances: 1,
  capabilities: [{
    browserName: "wry",
    "wdio:enforceWebDriverClassic": true,
    "tauri:options": {
      application: resolve(root, "target/release/video-compressor-desktop"),
    },
  }],
  logLevel: "info",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { timeout: 120000 },
};
