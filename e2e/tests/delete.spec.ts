import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { signedIn } from "../fixtures";

async function editFirstEntry(page: Page) {
  await installFakeBackend(page, signedIn());
  await page.goto("/");
  await expect(page.locator("#entry-e1")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Edit time entry" })).toBeVisible();
}

const deleteButton = (page: Page) => page.getByRole("button", { name: "Delete", exact: true });
const politeRegion = (page: Page) => page.locator('[role="status"][aria-live="polite"]').first();

test("the form's Delete fills up while held and deletes without asking", async ({ page }) => {
  await editFirstEntry(page);
  const button = deleteButton(page);
  await expect(button).toHaveAccessibleDescription("Hold for 2 seconds, or hold Space, to delete this entry");

  await button.hover();
  await page.mouse.down();
  await expect(button).toHaveClass(/holding/);
  await expect(page.locator(".delete-area .hold-hint")).toHaveText("Keep holding…");
  await page.screenshot({ path: "screenshots/hold-to-delete.png" });
  await expect.poll(() => invoked(page, "delete_entries"), { timeout: 4000 }).toEqual([{ ids: ["e1"] }]);
  await page.mouse.up();

  // Straight back to the list, without the entry and without a question.
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#entry-e1")).toHaveCount(0);
  await expect(page.getByRole("group", { name: /Delete/ })).toHaveCount(0);
});

test("a click on Delete says to hold it, and deletes nothing", async ({ page }) => {
  await editFirstEntry(page);
  await deleteButton(page).click();
  await expect(page.locator(".delete-area .hold-hint")).toBeVisible();
  await expect(politeRegion(page)).toHaveText("Hold to delete");
  await expect(deleteButton(page)).not.toHaveClass(/holding/);
  await expect(page.getByRole("heading", { name: "Edit time entry" })).toBeVisible();
  await expectAccessible(page);
  await page.waitForTimeout(2300);
  expect(await invoked(page, "delete_entries")).toHaveLength(0);
});

test("holding Space on Delete deletes too", async ({ page }) => {
  await editFirstEntry(page);
  await deleteButton(page).focus();
  await page.keyboard.down("Space");
  await expect(deleteButton(page)).toHaveClass(/holding/);
  await expect.poll(() => invoked(page, "delete_entries"), { timeout: 4000 }).toEqual([{ ids: ["e1"] }]);
  await page.keyboard.up("Space");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
});

test("letting go of Space early keeps the entry", async ({ page }) => {
  await editFirstEntry(page);
  await deleteButton(page).focus();
  await page.keyboard.down("Space");
  await page.waitForTimeout(600);
  await page.keyboard.up("Space");
  await expect(page.locator(".delete-area .hold-hint")).toBeVisible();
  await page.waitForTimeout(2000);
  expect(await invoked(page, "delete_entries")).toHaveLength(0);
  await expect(deleteButton(page)).toBeFocused();
});

test("assistive technology, which can't hold, is asked to confirm in the form", async ({ page }) => {
  await editFirstEntry(page);
  // A click no press started, like VoiceOver's.
  await deleteButton(page).dispatchEvent("click");
  const question = page.getByRole("group", { name: "Delete this entry?" });
  await expect(question).toBeVisible();
  await expect(question.getByRole("button", { name: "Delete" })).toBeFocused();
  await expectAccessible(page);

  await question.getByRole("button", { name: "Keep" }).click();
  await expect(deleteButton(page)).toBeFocused();
  expect(await invoked(page, "delete_entries")).toHaveLength(0);

  await deleteButton(page).dispatchEvent("click");
  await page.getByRole("group", { name: "Delete this entry?" }).getByRole("button", { name: "Delete" }).click();
  await expect.poll(() => invoked(page, "delete_entries")).toEqual([{ ids: ["e1"] }]);
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
});

test("dark mode: the filled Delete stays readable", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await editFirstEntry(page);
  await deleteButton(page).hover();
  await page.mouse.down();
  await page.waitForTimeout(1200);
  await page.screenshot({ path: "screenshots/hold-to-delete-dark.png" });
  await page.mouse.up();
  await expectAccessible(page);
});
