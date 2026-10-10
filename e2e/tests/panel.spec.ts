import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { type Fixture, signedIn } from "../fixtures";

async function open(page: Page, fixture: Fixture = signedIn()) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

const politeRegion = (page: Page) => page.locator('[role="status"][aria-live="polite"]').first();

test("shows today with its entries and the week totals", async ({ page }) => {
  await open(page);
  await expect(page.getByRole("tab", { name: "Friday, Oct 9, 2 hours, today" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const entries = page.locator(".entry-main");
  await expect(entries).toHaveCount(2);
  await expect(entries.first()).toHaveAccessibleName(
    "Design system › Header. Kuix. Header components. 1 hour 30 minutes, 09:00 to 10:30. billable",
  );
  await expect(page.locator(".week-total")).toContainText("10:00");
  await page.screenshot({ path: "screenshots/panel-light.png" });
  await expectAccessible(page);
});

test("focus starts on today's first entry; arrows move through entries and days", async ({ page }) => {
  await open(page);
  await expect(page.locator("#entry-e1")).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#entry-e2")).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(page.getByRole("heading", { name: "Thursday, Oct 8" })).toBeVisible();
  expect(await invoked(page, "select_date")).toContainEqual({ date: "2026-10-08" });
  await expect(page.getByRole("tab", { name: /Thursday, Oct 8/ })).toBeFocused();
  await page.keyboard.press("KeyT");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#entry-e1")).toBeFocused();
});

test("⌘N, a recent pair, ⌥2 and Enter add two hours", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  const picker = page.getByRole("button", { name: "Project and task" });
  await expect(picker).toBeFocused();
  await expect(picker).toHaveAccessibleName("Project and task No project");
  await page.keyboard.press("Meta+Digit2");
  await expect(picker).toHaveAccessibleName("Project and task Support · ACME");
  await page.keyboard.press("Alt+Digit2");
  await expect(page.getByRole("textbox", { name: "Duration" })).toHaveValue("2:00");
  await expect(page.getByRole("button", { name: "2 hours" })).toHaveAttribute("aria-pressed", "true");
  await page.getByLabel("Notes").fill("Call with ACME");
  await page.keyboard.press("Enter");

  await expect(page.locator(".entry-main")).toHaveCount(3);
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({
    id: null,
    date: "2026-10-09",
    project_id: "p3",
    task_id: null,
    duration: "2:00",
    description: "Call with ACME",
    start_time: "11:00",
    start_timer: false,
    billable: true,
  });
  await expect(politeRegion(page)).toHaveText("Added 2 hours");
});

test("typing on the project picker searches; Enter picks, then saves", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  await page.keyboard.type("head");
  const search = page.getByRole("combobox", { name: "Search projects and tasks" });
  await expect(search).toBeFocused();
  await expect(search).toHaveValue("head");
  const picker = page.getByRole("button", { name: "Project and task" });
  await expect(picker).toHaveAttribute("aria-expanded", "true");
  const listbox = page.getByRole("listbox", { name: "Projects and tasks" });
  await expect(listbox.getByRole("option")).toHaveCount(1);
  await expect(search).toHaveAttribute("aria-activedescendant", "work-option-0");
  await page.screenshot({ path: "screenshots/form-picker.png" });
  await expectAccessible(page);

  await page.keyboard.press("Enter");
  await expect(listbox).toBeHidden();
  await expect(picker).toBeFocused();
  await expect(picker).toHaveAccessibleName("Project and task Design system › Header · Kuix");
  await page.screenshot({ path: "screenshots/form.png" });
  await expectAccessible(page);

  // Enter on the closed picker saves, like anywhere else in the form.
  await page.keyboard.press("Alt+Digit1");
  await page.keyboard.press("Enter");
  await expect(page.locator(".entry-main")).toHaveCount(3);
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({ project_id: "p1", task_id: "t1", duration: "1:00" });
});

test("the project picker opens like a pop-up menu and Escape closes only the list", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  await page.keyboard.press("Space");
  const search = page.getByRole("combobox", { name: "Search projects and tasks" });
  await expect(search).toBeFocused();
  await expect(search).toHaveValue("");
  const options = page.getByRole("listbox", { name: "Projects and tasks" }).getByRole("option");
  await expect(options).toHaveCount(5);
  // It starts on the current choice.
  const active = await search.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toContainText("No project");

  await page.keyboard.press("Escape");
  const picker = page.getByRole("button", { name: "Project and task" });
  await expect(picker).toBeFocused();
  await expect(picker).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByRole("heading", { name: /New time entry/ })).toBeVisible();

  await picker.click();
  await options.filter({ hasText: "Support" }).click();
  await expect(picker).toHaveAccessibleName("Project and task Support · ACME");
  await expect(picker).toBeFocused();

  // Escape with the list closed leaves the form, and focus returns to the list.
  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#entry-e1")).toBeFocused();
});

test("projects created elsewhere show up while the form is open", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  expect(await invoked(page, "prepare_form")).toHaveLength(1);
  await page.evaluate(() => {
    const emit = (window as any).__emit;
    emit("view://catalog", {
      options: [
        { project_id: "p9", task_id: null, label: "Random projekt", client_name: null, color: "#ffa726", billable: false },
        { project_id: null, task_id: null, label: "No project", client_name: null, color: null, billable: false },
      ],
      recents: [],
    });
  });
  await page.keyboard.type("rand");
  const options = page.getByRole("listbox", { name: "Projects and tasks" }).getByRole("option");
  await expect(options).toHaveCount(1);
  await expect(options.first()).toContainText("Random projekt");
});

test("the form's buttons stay at the bottom while its fields scroll", async ({ page }) => {
  // A shorter panel than usual, so the fields don't fit.
  await page.setViewportSize({ width: 380, height: 440 });
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  const footer = page.locator(".form-footer");
  const pinned = (await footer.boundingBox())!;
  expect(pinned.y + pinned.height).toBeCloseTo(440, 0);

  const body = page.locator(".form-body");
  expect(await body.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await body.evaluate((element) => element.scrollTo(0, element.scrollHeight));
  // The last field ends above the buttons instead of sliding under them.
  const billable = (await page.locator(".form-row").boundingBox())!;
  expect(billable.y + billable.height).toBeLessThanOrEqual(pinned.y);

  // A duration error grows the fields, not the footer's position.
  await page.getByRole("textbox", { name: "Duration" }).fill("soon");
  await expect(page.locator("#duration-error")).toBeVisible();
  expect((await footer.boundingBox())!.y).toBe(pinned.y);

  // The project list is fixed under its button, so the scrolling fields don't clip it.
  await body.evaluate((element) => element.scrollTo(0, 0));
  await page.getByRole("button", { name: "Project and task" }).click();
  const button = (await page.getByRole("button", { name: "Project and task" }).boundingBox())!;
  const popup = (await page.locator("#work-popup").boundingBox())!;
  expect(popup.y).toBeCloseTo(button.y + button.height + 4, 0);
  await expect(page.getByRole("listbox", { name: "Projects and tasks" }).getByRole("option").last()).toBeInViewport();
  await page.screenshot({ path: "screenshots/form-short.png" });
});

test("an invalid duration is explained and nothing is saved", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+KeyN");
  const duration = page.getByRole("textbox", { name: "Duration" });
  await duration.fill("soon");
  await expect(duration).toHaveAttribute("aria-invalid", "true");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("alert").filter({ hasText: "Use a duration like" }).first()).toBeVisible();
  expect(await invoked(page, "save_entry")).toEqual([]);
});

test("⌘= and ⌘− change the selected entry by an hour", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+Equal");
  await expect(page.locator("#entry-e1 .duration")).toHaveText("2:30");
  await page.keyboard.press("Meta+Minus");
  await expect(page.locator("#entry-e1 .duration")).toHaveText("1:30");
  expect(await invoked(page, "adjust_entry")).toEqual([
    { id: "e1", delta_minutes: 60 },
    { id: "e1", delta_minutes: -60 },
  ]);
});

test("Backspace asks before deleting and Enter confirms", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Backspace");
  const confirm = page.getByRole("button", { name: "Delete ↩" });
  await expect(confirm).toBeFocused();
  await expectAccessible(page);
  await page.keyboard.press("Enter");
  await expect(page.locator(".entry-main")).toHaveCount(1);
  expect(await invoked(page, "delete_entry")).toEqual([{ id: "e1" }]);
});

test("Escape hides the panel", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Escape");
  expect(await invoked(page, "panel_hide")).toHaveLength(1);
});

test("settings open with ⌘, and close with Escape", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Meta+Comma");
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Show or hide Solidbar ⌥⌘T", exact: true })).toBeVisible();
  await page.screenshot({ path: "screenshots/settings.png", fullPage: true });
  await expectAccessible(page);
  await page.keyboard.press("Escape");
  await expect(page.locator("#entry-e1")).toBeFocused();
});

test("dark mode stays readable", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await open(page);
  await page.screenshot({ path: "screenshots/panel-dark.png" });
  await expectAccessible(page);
  await page.keyboard.press("Meta+KeyN");
  await page.screenshot({ path: "screenshots/form-dark.png" });
  await expectAccessible(page);
});

test("increased contrast stays readable", async ({ page }) => {
  await page.emulateMedia({ contrast: "more" });
  await open(page);
  await expectAccessible(page);
});

test("offers Launch at login once after signing in", async ({ page }) => {
  const fixture = signedIn();
  fixture.app.settings.launch_at_login_offered = false;
  await open(page, fixture);
  const offer = page.getByRole("region", { name: "Launch at login" });
  await expect(offer).toBeVisible();
  await expectAccessible(page);
  await offer.getByRole("button", { name: "Launch at login" }).click();
  expect(await invoked(page, "set_autostart")).toEqual([{ enabled: true }]);
});
