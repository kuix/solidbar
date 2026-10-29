import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { type Fixture, signedIn } from "../fixtures";

async function openPanel(page: Page, fixture: Fixture = signedIn()) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

test("☆ keeps the form as a favorite, and ⌃1 starts a new entry from it", async ({ page }) => {
  await openPanel(page);
  await page.keyboard.press("Meta+KeyN");
  await expect(page.getByRole("group", { name: "Favorites" })).toHaveCount(0);
  await page.keyboard.press("Meta+Digit1");
  await page.getByRole("textbox", { name: "Notes" }).fill("Weekly");
  await page.keyboard.press("Alt+Digit1");
  const star = page.getByRole("button", { name: "Favorite", exact: true });
  await expect(star).toHaveAttribute("aria-pressed", "false");
  await page.keyboard.press("Meta+KeyD");
  await expect(star).toHaveAttribute("aria-pressed", "true");
  expect(await invoked(page, "set_favorite")).toEqual([
    { project_id: "p1", task_id: "t1", description: "Weekly", duration: "1:00", billable: true, favorite: true },
  ]);
  await page.getByRole("button", { name: "Cancel" }).click();

  await page.keyboard.press("Meta+KeyN");
  const weekly = page
    .getByRole("group", { name: "Favorites" })
    .getByRole("button", { name: "Weekly, Design system › Header, 1 hour" });
  await expect(weekly).toHaveAttribute("aria-keyshortcuts", "Control+1");
  await expect(star).toHaveAttribute("aria-pressed", "false");
  await expectAccessible(page);

  await page.keyboard.press("Control+Digit1");
  await expect(page.getByRole("button", { name: /^Project and task/ })).toBeFocused();
  await expect(page.getByRole("textbox", { name: "Notes" })).toHaveValue("Weekly");
  await expect(page.getByRole("textbox", { name: "Duration" })).toHaveValue("1:00");
  await expect(star).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Enter");
  await expect.poll(() => invoked(page, "save_entry")).toHaveLength(1);
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({
    project_id: "p1",
    task_id: "t1",
    description: "Weekly",
    duration: "1:00",
    billable: true,
  });
});

test("a click fills the form in with a favorite, and ☆ again forgets it", async ({ page }) => {
  const fixture = signedIn();
  const support = fixture.catalog.options.find((option: any) => option.project_id === "p3");
  fixture.catalog.favorites = [{ option: support, description: "", duration: "0:30", billable: false }];
  await openPanel(page, fixture);
  await page.keyboard.press("Meta+KeyN");

  await page.getByRole("button", { name: "Support, 30 minutes" }).click();
  await expect(page.getByRole("button", { name: /^Project and task/ })).toHaveAccessibleName(/Support/);
  await expect(page.getByRole("textbox", { name: "Duration" })).toHaveValue("0:30");
  await expect(page.getByRole("switch", { name: "Billable" })).not.toBeChecked();

  const star = page.getByRole("button", { name: "Favorite", exact: true });
  await expect(star).toHaveAttribute("aria-pressed", "true");
  await star.click();
  await expect(star).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("group", { name: "Favorites" })).toHaveCount(0);
  expect(await invoked(page, "set_favorite")).toContainEqual(
    expect.objectContaining({ project_id: "p3", task_id: null, favorite: false }),
  );
  // Only the favorite is gone; nothing was saved.
  expect(await invoked(page, "save_entry")).toHaveLength(0);
});

test("editing an entry offers the star but not the favorites", async ({ page }) => {
  const fixture = signedIn();
  fixture.catalog.favorites = [
    { option: fixture.catalog.recents[0], description: "Weekly", duration: "1:00", billable: true },
  ];
  await openPanel(page, fixture);
  await expect(page.locator("#entry-e1")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Edit time entry" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Favorite", exact: true })).toBeVisible();
  await expect(page.getByRole("group", { name: "Favorites" })).toHaveCount(0);
});
