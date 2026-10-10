import { defineConfig } from "@playwright/test";

// The UI runs in WKWebView, so test it in WebKit at the panel's size, with the Tauri IPC
// replaced by the fake backend in `fake-backend.ts`.
export default defineConfig({
  testDir: "./tests",
  outputDir: "./test-results",
  fullyParallel: true,
  reporter: [["list"]],
  // The first page load waits for a cold wasm build on slow machines.
  expect: { timeout: 10_000 },
  use: {
    browserName: "webkit",
    viewport: { width: 380, height: 520 },
  },
  projects: [
    { name: "ui", testIgnore: /release\.spec\.ts/, use: { baseURL: "http://127.0.0.1:1421" } },
    // The release build under the packaged app's Content-Security-Policy.
    { name: "release", testMatch: /release\.spec\.ts/, use: { baseURL: "http://127.0.0.1:1422" } },
  ],
  webServer: [
    {
      command: "trunk serve --port 1421 --no-autoreload",
      cwd: "../crates/ui",
      url: "http://127.0.0.1:1421",
      reuseExistingServer: true,
      timeout: 600_000,
    },
    {
      command:
        "trunk build --release --dist ../../e2e/.release-dist && node ../../e2e/csp-server.mjs ../../e2e/.release-dist 1422",
      cwd: "../crates/ui",
      url: "http://127.0.0.1:1422",
      reuseExistingServer: true,
      timeout: 600_000,
    },
  ],
});
