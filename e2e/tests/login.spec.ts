import { expect, test } from "@playwright/test";
import { installFakeBackend, invoked } from "../fake-backend";
import { signedOut, signingIn } from "../fixtures";
import { expectAccessible } from "../a11y";

test("the sign-in form is labelled, names the redirect URIs and starts the sign-in", async ({ page }) => {
  await installFakeBackend(page, signedOut());
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Sign in to solidtime" })).toBeVisible();
  await expect(page.getByLabel("solidtime address")).toBeFocused();
  // No choice of redirect or port: Solidbar works out what the client allows.
  await expect(page.getByRole("radio")).toHaveCount(0);
  await expect(page.getByRole("spinbutton")).toHaveCount(0);
  await expect(page.getByLabel("OAuth client ID")).toHaveAccessibleDescription(
    /http:\/\/127\.0\.0\.1\/callback or solidtime:\/\/oauth\/callback/,
  );
  await page.screenshot({ path: "screenshots/login.png" });
  await expectAccessible(page);

  await page.getByLabel("solidtime address").fill("https://time.kuix.hu");
  await page.getByLabel("OAuth client ID").fill("client-9");
  await page.getByRole("button", { name: "Sign in with browser" }).click();
  await expect
    .poll(() => invoked(page, "auth_login"))
    .toEqual([{ url: "https://time.kuix.hu", client_id: "client-9" }]);
});

test("signing in says what it's waiting for", async ({ page }) => {
  await installFakeBackend(page, signingIn(null));
  await page.goto("/");
  await expect(page.getByRole("status").filter({ hasText: "Checking the OAuth client…" })).toBeVisible();
  await expect(page.getByLabel("solidtime address")).toBeDisabled();
  await expect(page.getByRole("button", { name: "Cancel" })).toBeVisible();
});

test("returning through the solidtime:// link warns about the official app", async ({ page }) => {
  await installFakeBackend(page, signingIn({ mode: "deep_link" }));
  await page.goto("/");
  await expect(page.getByRole("status").and(page.locator(".waiting"))).toHaveText("Waiting for you to approve Solidbar in the browser…");
  await expect(page.getByText("If the official solidtime app opens instead")).toBeVisible();
  await expectAccessible(page);
});

test("returning to the local address needs no warning", async ({ page }) => {
  await installFakeBackend(page, signingIn({ mode: "loopback", port: 52107 }));
  await page.goto("/");
  await expect(page.getByRole("status").and(page.locator(".waiting"))).toHaveText("Waiting for you to approve Solidbar in the browser…");
  await expect(page.getByText("If the official solidtime app opens instead")).toHaveCount(0);
});

test("a failed sign-in is announced", async ({ page }) => {
  const error = "solidtime doesn't accept this OAuth client ID. Check the ID, and that the client allows http://127.0.0.1/callback or solidtime://oauth/callback as its redirect URI.";
  await installFakeBackend(page, signedOut(error));
  await page.goto("/");
  await expect(page.getByRole("alert").filter({ hasText: "doesn't accept this OAuth client ID" })).toBeVisible();
});
