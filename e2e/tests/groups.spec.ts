import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { continued, type Fixture } from "../fixtures";

async function open(page: Page, fixture: Fixture = continued()) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
}

test("a continued entry shares its row; → shows its entries and ← hides them", async ({ page }) => {
  await open(page);
  const rows = page.locator(".entries > .entry");
  await expect(rows).toHaveCount(2);
  const group = page.locator("#entry-e1");
  await expect(group).toBeFocused();
  await expect(group).toHaveAccessibleName(
    "Design system › Header. Kuix. Header components. 1 hour 50 minutes, 2 entries, 09:00 to 11:20. billable",
  );
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await expect(group.locator(".duration")).toHaveText("1:50");

  await page.keyboard.press("ArrowRight");
  await expect(group).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await expect(page.locator("#interval-e1")).toHaveAccessibleName("09:00 to 10:30, 1 hour 30 minutes");
  await page.screenshot({ path: "screenshots/group-open.png" });
  await expectAccessible(page);

  // ↓ walks into the group's entries, and one opens in the form on its own.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#interval-e3")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: /Edit time entry/ })).toBeVisible();
  expect((await invoked(page, "prepare_form")).at(-1)).toMatchObject({ entry_id: "e3", group_ids: [] });
  await page.keyboard.press("Escape");
  await expect(page.locator("#interval-e3")).toBeFocused();

  // ← from an entry closes its group, and ← on a closed row changes the day as usual.
  await page.keyboard.press("ArrowLeft");
  await expect(group).toBeFocused();
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator("#interval-e1")).toHaveCount(0);
  await page.keyboard.press("ArrowLeft");
  await expect(page.getByRole("heading", { name: "Thursday, Oct 8" })).toBeVisible();
});

test("the count opens the group with the mouse", async ({ page }) => {
  await open(page);
  await page.locator("#entry-e1 .count").click();
  await expect(page.locator("#entry-e1")).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("#interval-e3")).toBeVisible();
  await expect.poll(() => invoked(page, "prepare_form")).toEqual([]);
});

test("a group opens in the form with what its entries share", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: /Edit 2 entries/ })).toBeVisible();
  await expect.poll(() => invoked(page, "prepare_form")).toEqual([
    { date: "2026-10-09", entry_id: null, group_ids: ["e1", "e3"] },
  ]);
  await expect(page.getByText("Changes apply to all 2 entries (1:50 in total)")).toBeVisible();
  // Their times stay as they are.
  await expect(page.getByRole("textbox", { name: "Duration" })).toHaveCount(0);
  await expect(page.getByLabel("Start")).toHaveCount(0);
  await expectAccessible(page);

  await page.getByLabel("Notes").fill("Header and footer");
  await page.keyboard.press("Enter");
  const [draft] = await invoked(page, "save_entry");
  expect(draft).toMatchObject({
    id: null,
    group_ids: ["e1", "e3"],
    description: "Header and footer",
    project_id: "p1",
    task_id: "t1",
    kind: "work",
  });
});

test("deleting a group asks once for all of its entries", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Backspace");
  await expect(page.getByRole("group", { name: "Delete these 2 entries?" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Delete ↩" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect.poll(() => invoked(page, "delete_entries")).toEqual([{ ids: ["e1", "e3"] }]);
  await expect(page.locator(".entries > .entry")).toHaveCount(1);
});

test("S continues the group, and −/+ change its latest entry", async ({ page }) => {
  await open(page);
  await page.keyboard.press("KeyS");
  await expect.poll(() => invoked(page, "start_timer")).toEqual([{ kind: "entry", id: "e3" }]);
  await page.keyboard.press("Meta+Equal");
  await expect.poll(() => invoked(page, "adjust_entry")).toEqual([{ id: "e3", delta_minutes: 60 }]);
});
