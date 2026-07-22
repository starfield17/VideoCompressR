import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const config = {
  runner: "local",
  specs: [resolve(root, "e2e/**/*.e2e.mjs")],
  maxInstances: 1,
  capabilities: [{
    browserName: "tauri",
    "tauri:options": {
      application: resolve(root, "target/release/video-compressor-desktop"),
    },
  }],
  logLevel: "info",
  framework: "mocha",
  reporters: ["spec"],
  services: [["@wdio/tauri-service", { driverProvider: "external" }]],
  mochaOpts: { timeout: 120000 },
};
