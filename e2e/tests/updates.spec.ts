import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { type Fixture, signedIn } from "../fixtures";

async function open(page: Page, fixture: Fixture) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

test("a found update shows a banner that installs it", async ({ page }) => {
  await open(page, {
    ...signedIn(),
    update: { current_version: "0.1.0", state: { state: "available", version: "0.2.0" } },
  });
  const banner = page.getByRole("region", { name: "Update" });
  await expect(banner).toContainText("Solidbar 0.2.0 is available.");
  await expectAccessible(page);

  await banner.getByRole("button", { name: "Restart to update" }).click();
  expect(await invoked(page, "update_install")).toHaveLength(1);
  await expect(banner).toContainText("Downloading Solidbar 0.2.0… 40%");
});

test("Settings checks for updates and says the result", async ({ page }) => {
  await open(page, signedIn());
  await expect(page.getByRole("region", { name: "Update" })).toHaveCount(0);
  await page.keyboard.press("Meta+Comma");

  const button = page.getByRole("button", { name: "Check for updates" });
  await expect(button).toHaveAccessibleDescription("Version 0.1.0");
  await button.click();
  await expect(page.locator('[role="status"][aria-live="polite"]').first()).toHaveText("Solidbar is up to date");
  await expect(page.locator("#update-status")).toHaveText("Version 0.1.0 · Up to date");
  await expectAccessible(page);
});

test("Settings installs an update the check found", async ({ page }) => {
  await open(page, {
    ...signedIn(),
    updateCheck: { state: "available", version: "0.2.0" },
  });
  await page.keyboard.press("Meta+Comma");
  await page.getByRole("button", { name: "Check for updates" }).click();
  // The banner stays out of Settings, which shows the same thing.
  await expect(page.getByRole("region", { name: "Update" })).toHaveCount(0);

  const install = page.getByRole("button", { name: "Install and restart" });
  await install.click();
  expect(await invoked(page, "update_install")).toHaveLength(1);
  await expect(page.getByRole("button", { name: "Check for updates" })).toHaveAttribute("aria-disabled", "true");
  await expect(page.locator("#update-status")).toHaveText("Downloading Solidbar 0.2.0… 40%");
});
