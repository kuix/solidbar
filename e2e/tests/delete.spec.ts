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

test("the form's Delete asks first, and deletes back in the list", async ({ page }) => {
  await editFirstEntry(page);
  await deleteButton(page).click();
  const question = page.getByRole("group", { name: "Delete this entry?" });
  await expect(question).toBeVisible();
  await expect(question.getByRole("button", { name: "Delete" })).toBeFocused();
  await page.screenshot({ path: "screenshots/delete-form.png" });
  await expectAccessible(page);
  expect(await invoked(page, "delete_entries")).toHaveLength(0);

  await page.keyboard.press("Enter");
  await expect.poll(() => invoked(page, "delete_entries")).toEqual([{ ids: ["e1"] }]);
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#entry-e1")).toHaveCount(0);
});

test("Keep or ⎋ keeps the entry, and the form open", async ({ page }) => {
  await editFirstEntry(page);
  const question = page.getByRole("group", { name: "Delete this entry?" });
  await deleteButton(page).click();
  await question.getByRole("button", { name: "Keep" }).click();
  await expect(question).toHaveCount(0);
  await expect(deleteButton(page)).toBeFocused();

  await page.keyboard.press("Space");
  await expect(question).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(question).toHaveCount(0);
  await expect(deleteButton(page)).toBeFocused();
  await expect(page.getByRole("heading", { name: "Edit time entry" })).toBeVisible();
  expect(await invoked(page, "delete_entries")).toHaveLength(0);
});

test("dark mode: the question stays readable", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await editFirstEntry(page);
  await deleteButton(page).click();
  await expect(page.getByRole("group", { name: "Delete this entry?" })).toBeVisible();
  await page.screenshot({ path: "screenshots/delete-form-dark.png" });
  await expectAccessible(page);
});
