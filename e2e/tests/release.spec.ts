import { expect, test } from "@playwright/test";
import { installFakeBackend } from "../fake-backend";
import { signedIn } from "../fixtures";

test("the release build starts under the app's Content-Security-Policy", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await installFakeBackend(page, signedIn());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible({ timeout: 10_000 });
  expect(errors).toEqual([]);
});
