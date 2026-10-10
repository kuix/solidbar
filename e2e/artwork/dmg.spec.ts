import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "@playwright/test";

const here = path.dirname(fileURLToPath(import.meta.url));
const output = path.resolve(here, "../../src-tauri/dmg");

test.use({ viewport: { width: 660, height: 400 } });

// One TIFF with a 1x and a 2x image, so the background stays sharp on Retina screens.
test("DMG background", async ({ browser }, testInfo) => {
  const sizes: string[] = [];
  for (const scale of [1, 2]) {
    const page = await browser.newPage({ viewport: { width: 660, height: 400 }, deviceScaleFactor: scale });
    await page.goto(`file://${path.join(here, "dmg-background.html")}`);
    const file = testInfo.outputPath(`background@${scale}x.png`);
    await page.screenshot({ path: file });
    sizes.push(file);
    await page.close();
  }
  mkdirSync(output, { recursive: true });
  execFileSync("tiffutil", ["-cathidpicheck", ...sizes, "-out", path.join(output, "background.tiff")]);
});
