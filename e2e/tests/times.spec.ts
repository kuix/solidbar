import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { type Fixture, signedIn } from "../fixtures";

async function openForm(page: Page, fixture: Fixture = signedIn()) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await page.keyboard.press("Meta+KeyN");
  return page.getByRole("textbox", { name: "Start" });
}

test("start times are typed in 24-hour form, without AM/PM", async ({ page }) => {
  const start = await openForm(page);
  await expect(start).toHaveValue("11:00");
  await start.fill("930");
  await start.press("Tab");
  await expect(start).toHaveValue("09:30");
  await start.press("ArrowUp");
  await expect(start).toHaveValue("09:45");
  await start.press("Shift+ArrowDown");
  await expect(start).toHaveValue("08:45");

  await page.keyboard.press("Alt+Digit1");
  await page.getByRole("button", { name: "Save" }).click();
  await expect.poll(() => invoked(page, "save_entry")).toHaveLength(1);
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({ start_time: "08:45", duration: "1:00" });
});

test("a start time that isn't one is explained and nothing is saved", async ({ page }) => {
  const start = await openForm(page);
  await start.fill("25");
  await start.press("Tab");
  await expect(start).toHaveAttribute("aria-invalid", "true");
  await expect(start).toHaveAccessibleDescription("Enter a time like 09:30");
  await expectAccessible(page);

  await page.keyboard.press("Alt+Digit2");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.locator(".form-footer").getByRole("alert")).toHaveText("Enter when the entry starts");
  await expect(start).toBeFocused();
  expect(await invoked(page, "save_entry")).toHaveLength(0);
});

test("with a 12-hour clock, times show and accept AM/PM", async ({ page }) => {
  const fixture = signedIn();
  fixture.app.hour12 = true;
  const start = await openForm(page, fixture);
  await expect(start).toHaveValue("11:00 AM");
  await start.fill("2:30 pm");
  await start.press("Tab");
  await expect(start).toHaveValue("2:30 PM");
  await page.keyboard.press("Alt+Digit1");
  await page.getByRole("button", { name: "Save" }).click();
  await expect.poll(() => invoked(page, "save_entry")).toHaveLength(1);
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({ start_time: "14:30" });
});

test("Settings chooses the time format, following macOS by default", async ({ page }) => {
  await installFakeBackend(page, signedIn());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await page.keyboard.press("Meta+Comma");
  const group = page.getByRole("group", { name: "Time format" });
  await expect(group.getByRole("radio", { name: "Match macOS" })).toBeChecked();
  await group.getByRole("radio", { name: "12-hour (2:30 PM)" }).check();
  expect(await invoked(page, "update_settings")).toContainEqual(expect.objectContaining({ clock: "12h" }));

  const workdayStart = page.getByRole("textbox", { name: "New days start at" });
  await expect(workdayStart).toHaveValue("09:00");
  await workdayStart.fill("8:30");
  await workdayStart.press("Tab");
  expect(await invoked(page, "update_settings")).toContainEqual(
    expect.objectContaining({ workday_start: "08:30" }),
  );
  await expectAccessible(page);
});
