import { defineConfig } from "@playwright/test";

// Renders the artwork: the README images (screenshots of the current UI and the cover) into
// docs/images, and the DMG window background into src-tauri/dmg. `npm run artwork`; not part of
// the test run.
export default defineConfig({
  testDir: "./artwork",
  outputDir: "./test-results-artwork",
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:1421",
    browserName: "webkit",
    viewport: { width: 380, height: 520 },
    deviceScaleFactor: 2,
    reducedMotion: "reduce",
  },
  webServer: {
    command: "trunk serve --port 1421 --no-autoreload",
    cwd: "../crates/ui",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: true,
    timeout: 600_000,
  },
});
