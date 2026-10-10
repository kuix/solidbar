import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { continued, type Fixture, onBreak, signedIn, withBreaks } from "../fixtures";

async function open(page: Page, fixture: Fixture) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

test("a break shows apart from the work time, with a way back to work", async ({ page }) => {
  await open(page, onBreak());
  await expect(page.locator(".day-heading-total .sr-only")).toHaveText(
    "1 hour 30 minutes worked, 15 minutes of breaks",
  );
  await expect(page.locator("#entry-pause")).toHaveAccessibleName(
    "Break. 15 minutes, 10:30 to now. on a break now",
  );
  const banner = page.getByRole("region", { name: "Break" });
  await expect(banner.getByRole("button", { name: "Resume “Header components”" })).toBeVisible();
  await page.screenshot({ path: "screenshots/break.png" });
  await expectAccessible(page);

  await banner.getByRole("button", { name: /Resume/ }).click();
  await expect.poll(() => invoked(page, "start_timer")).toEqual([{ kind: "resume" }]);
});

test("during a break S goes back to work and B ends the break", async ({ page }) => {
  await open(page, onBreak());
  // On a work row, S continues that row.
  await expect(page.locator("#entry-e1")).toBeFocused();
  await page.keyboard.press("KeyS");
  // On the break itself, S goes back to the work before it.
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#entry-pause")).toBeFocused();
  await page.keyboard.press("KeyS");
  await expect.poll(() => invoked(page, "start_timer")).toEqual([{ kind: "entry", id: "e1" }, { kind: "resume" }]);
  await page.keyboard.press("KeyB");
  await expect.poll(() => invoked(page, "stop_timer")).toHaveLength(1);
  await expect.poll(() => invoked(page, "start_break")).toEqual([]);
});

test("B takes a break while the timer runs", async ({ page }) => {
  const fixture = withBreaks(continued());
  const friday = fixture.app.week.days.find((day: any) => day.date === "2026-10-09");
  Object.assign(friday.entries[2], { end_label: null, running: true });
  await open(page, fixture);
  await expect(page.locator("#entry-e1").locator("..").getByRole("button", { name: "Take a break" })).toBeVisible();
  await page.keyboard.press("KeyB");
  await expect.poll(() => invoked(page, "start_break")).toHaveLength(1);
});

test("without breaks in the organization, B does nothing and the picker has no Break", async ({ page }) => {
  await open(page, signedIn());
  await page.keyboard.press("KeyB");
  await expect.poll(() => invoked(page, "start_break")).toEqual([]);
  await page.keyboard.press("Meta+KeyN");
  await expect(page.getByRole("button", { name: "Project and task" })).toBeFocused();
  await page.keyboard.type("break");
  await expect(page.getByRole("listbox", { name: "Projects and tasks" }).getByRole("option")).toHaveCount(0);
});

test("a break can be added in the form, and isn't billable", async ({ page }) => {
  await open(page, withBreaks(signedIn()));
  await page.keyboard.press("Meta+KeyN");
  const picker = page.getByRole("button", { name: "Project and task" });
  // Typed before the form is there, "b" would take a break in the list.
  await expect(picker).toBeFocused();
  await page.keyboard.type("break");
  await page.keyboard.press("Enter");
  await expect(picker).toHaveAccessibleName("Project and task Break");
  const billable = page.getByRole("switch", { name: "Billable" });
  await expect(billable).toBeDisabled();
  await expect(billable).not.toBeChecked();
  await expectAccessible(page);
  await page.keyboard.press("Alt+Digit1");
  await page.keyboard.press("Enter");
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({ kind: "break", project_id: null, billable: false, duration: "1:00" });
});
