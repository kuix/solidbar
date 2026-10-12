import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { signedIn } from "../fixtures";

async function open(page: Page) {
  await installFakeBackend(page, signedIn());
  await page.goto("/");
  await expect(page.locator("#entry-e1")).toBeFocused();
}

const newEntryButton = (page: Page) => page.getByRole("button", { name: "New entry" });
const todayButton = (page: Page) => page.getByRole("button", { name: "Today" });

test("in the list a single key is enough, and ⌘N and ⌘R still work", async ({ page }) => {
  await open(page);
  await expect(newEntryButton(page)).toHaveAttribute("aria-keyshortcuts", "N Meta+N");
  await expect(newEntryButton(page).locator("kbd")).toHaveText("N");
  await expect(todayButton(page)).toHaveAttribute("aria-keyshortcuts", "T");
  await expect(page.locator("#entry-e1")).toHaveAttribute("aria-keyshortcuts", /\bE S\b/);

  await page.keyboard.press("KeyN");
  await expect(page.getByRole("heading", { name: /New time entry/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#entry-e1")).toBeFocused();
  await page.keyboard.press("Meta+KeyN");
  await expect(page.getByRole("heading", { name: /New time entry/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#entry-e1")).toBeFocused();

  await page.keyboard.press("KeyR");
  await page.keyboard.press("Meta+KeyR");
  await expect.poll(() => invoked(page, "refresh")).toEqual([{ force: true }, { force: true }]);
});

test("single-key shortcuts can be turned off in Settings", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+Comma");
  const toggle = page.getByRole("switch", { name: "Single-key shortcuts" });
  await expect(toggle).toBeChecked();
  await expect(toggle).toHaveAccessibleDescription(/Voice Control/);
  await toggle.click();
  await expect(toggle).not.toBeChecked();
  expect(await invoked(page, "update_settings")).toContainEqual(
    expect.objectContaining({ single_key_shortcuts: false }),
  );
  await expectAccessible(page);
  await page.keyboard.press("Escape");
  await expect(page.locator("#entry-e1")).toBeFocused();

  // The hints show only what works.
  await expect(newEntryButton(page)).toHaveAttribute("aria-keyshortcuts", "Meta+N");
  await expect(newEntryButton(page).locator("kbd")).toHaveText("⌘N");
  await expect(todayButton(page)).not.toHaveAttribute("aria-keyshortcuts");
  await expect(todayButton(page)).toHaveAttribute("title", "Go to today");
  await expect(page.locator("#entry-e1")).toHaveAttribute(
    "aria-keyshortcuts",
    "Enter Meta+Equal Meta+Minus Backspace",
  );

  // Letters do nothing; the modifier versions, ↩ and the arrows still do.
  for (const key of ["KeyN", "KeyE", "KeyS", "KeyR", "KeyT"]) await page.keyboard.press(key);
  await expect(page.locator("#entry-e1")).toBeFocused();
  expect(await invoked(page, "prepare_form")).toHaveLength(0);
  expect(await invoked(page, "start_timer")).toHaveLength(0);
  expect(await invoked(page, "refresh")).toHaveLength(0);
  await page.keyboard.press("Meta+KeyN");
  await expect(page.getByRole("heading", { name: /New time entry/ })).toBeVisible();
});
