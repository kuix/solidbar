import { expect, test } from "@playwright/test";
import { installFakeBackend, invoked } from "../fake-backend";
import { signedOut } from "../fixtures";
import { expectAccessible } from "../a11y";

test("the sign-in form is labelled, shows the redirect URI and starts the sign-in", async ({ page }) => {
  await installFakeBackend(page, signedOut());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Sign in to solidtime" })).toBeVisible();
  await expect(page.getByLabel("solidtime address")).toBeFocused();
  await expect(page.getByLabel("Local address (self-hosted)")).toBeChecked();
  await expect(page.locator("#redirect-help")).toContainText("http://127.0.0.1:47821/callback");
  await page.screenshot({ path: "screenshots/login.png" });
  await expectAccessible(page);

  await page.getByLabel("solidtime address").fill("https://time.kuix.hu");
  await page.getByLabel("OAuth client ID").fill("client-9");
  await page.getByRole("button", { name: "Sign in with browser" }).click();
  expect(await invoked(page, "auth_login")).toEqual([
    { url: "https://time.kuix.hu", client_id: "client-9", redirect: { mode: "loopback", port: 47821 } },
  ]);
});

test("a failed sign-in is announced", async ({ page }) => {
  await installFakeBackend(page, signedOut("Port 47821 is already in use."));
  await page.goto("/");
  await expect(page.getByRole("alert").filter({ hasText: "Port 47821 is already in use." })).toBeVisible();
});
