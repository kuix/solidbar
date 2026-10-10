import path from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { installFakeBackend } from "../fake-backend";
import { showcase } from "./showcase";

const here = path.dirname(fileURLToPath(import.meta.url));
const images = path.resolve(here, "../../docs/images");

// The panel on an opaque material and with transparent corners, as it looks over a desktop.
async function open(page: Page, scheme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
  await installFakeBackend(page, showcase());
  await page.addInitScript(() => {
    const style = document.createElement("style");
    style.textContent = `
      html { background: transparent !important; }
      :root { --panel: #f4f4f6 !important; }
      @media (prefers-color-scheme: dark) { :root { --panel: #222225 !important; } }`;
    document.addEventListener("DOMContentLoaded", () => document.head.append(style));
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

async function shoot(page: Page, name: string) {
  await page.screenshot({ path: path.join(images, name), omitBackground: true });
}

test("today", async ({ page }) => {
  for (const scheme of ["light", "dark"] as const) {
    await open(page, scheme);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await shoot(page, `today-${scheme}.png`);
  }
});

test("new entry", async ({ page }) => {
  for (const scheme of ["light", "dark"] as const) {
    await open(page, scheme);
    await page.keyboard.press("Meta+KeyN");
    await page.keyboard.type("de");
    await expect(page.getByRole("listbox", { name: "Projects and tasks" })).toBeVisible();
    await shoot(page, `new-entry-${scheme}.png`);
  }
});

test("settings", async ({ page }) => {
  for (const scheme of ["light", "dark"] as const) {
    await open(page, scheme);
    await page.keyboard.press("Meta+Comma");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await shoot(page, `settings-${scheme}.png`);
  }
});

test.describe("cover", () => {
  test.use({ viewport: { width: 1280, height: 640 }, deviceScaleFactor: 1.5 });
  test("cover", async ({ page }) => {
    // Runs after the screenshots it shows.
    await page.goto(`file://${path.join(here, "cover.html")}`);
    await page.waitForFunction(() => [...document.images].every((image) => image.complete && image.naturalWidth > 0));
    await page.screenshot({ path: path.join(images, "cover.png") });
  });
});
