import { expect, test } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { choosingOrg, signedIn } from "../fixtures";

test("with several organizations, the first sign-in asks which one to use", async ({ page }) => {
  await installFakeBackend(page, choosingOrg());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Choose an organization" })).toBeVisible();
  const list = page.getByRole("list", { name: "Choose an organization" });
  await expect(list.getByRole("button")).toHaveText(["ACME", "Kuix"]);
  await expect(page.getByRole("button", { name: "ACME" })).toBeFocused();
  // Nothing else to do before choosing: no week, no new entry.
  await expect(page.getByRole("tablist")).toHaveCount(0);
  await page.keyboard.press("Meta+KeyN");
  expect(await invoked(page, "prepare_form")).toHaveLength(0);
  await expectAccessible(page);

  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "Kuix" })).toBeFocused();
  await page.keyboard.press("Enter");
  expect(await invoked(page, "select_org")).toEqual([{ membership_id: "m1" }]);
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#entry-e1")).toBeFocused();
});

test("the organization is changed in Settings, which ⌘O opens", async ({ page }) => {
  await installFakeBackend(page, signedIn());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  // Not in the header any more.
  await expect(page.getByRole("combobox", { name: "Organization" })).toHaveCount(0);

  await page.keyboard.press("Meta+KeyO");
  const select = page.getByLabel("Organization for new entries");
  await expect(select).toBeFocused();
  await select.selectOption({ label: "ACME" });
  expect(await invoked(page, "select_org")).toEqual([{ membership_id: "m2" }]);
});
