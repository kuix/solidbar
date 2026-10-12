import { expect, test, type Page } from "@playwright/test";
import { expectAccessible } from "../a11y";
import { installFakeBackend, invoked } from "../fake-backend";
import { type Fixture, signedIn } from "../fixtures";

async function openSettings(page: Page, fixture: Fixture = signedIn()) {
  await installFakeBackend(page, fixture);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Friday, Oct 9" })).toBeVisible();
  await page.keyboard.press("Meta+Comma");
}

const copied = async (page: Page) => (await invoked(page, "copy_text")).map((args: any) => args.text);

test("AI agents stay off until turned on; then Settings gives what agents need", async ({ page }) => {
  await openSettings(page);
  const access = page.getByRole("switch", { name: "Let AI agents use Solidbar" });
  await expect(access).not.toBeChecked();
  await expect(access).toHaveAccessibleDescription(/list your projects and time entries, add entries and change them/);
  await expect(page.getByLabel("Address", { exact: true })).toHaveCount(0);

  await access.click();
  expect(await invoked(page, "set_agent_access")).toEqual([{ enabled: true }]);
  await expect(access).toBeChecked();
  const address = page.getByLabel("Address", { exact: true });
  const key = page.getByLabel("Key", { exact: true });
  await expect(address).toHaveValue("http://127.0.0.1:52731/mcp");
  await expect(key).toHaveValue("k3y-1");
  await address.scrollIntoViewIfNeeded();
  await page.screenshot({ path: "screenshots/settings-agents.png" });
  await expectAccessible(page);

  await page.getByRole("button", { name: "Copy the address" }).click();
  await page.getByRole("button", { name: "Copy for Claude Code" }).click();
  await page.getByRole("button", { name: "Copy for Claude Desktop" }).click();
  const [url, command, desktop] = await copied(page);
  expect(url).toBe("http://127.0.0.1:52731/mcp");
  expect(command).toBe(
    'claude mcp add --transport http --scope user solidbar http://127.0.0.1:52731/mcp --header "Authorization: Bearer k3y-1"',
  );
  expect(JSON.parse(desktop).mcpServers.solidbar).toEqual({
    command: "/Applications/Solidbar.app/Contents/MacOS/solidbar",
    args: ["mcp"],
  });
  await expect(page.locator('[role="status"][aria-live="polite"]').first()).toHaveText(
    "Copied the setup for Claude Desktop",
  );

  // A new key takes the old one's place, without moving focus.
  const newKey = page.getByRole("button", { name: "New key" });
  await newKey.focus();
  await page.keyboard.press("Enter");
  await expect(key).toHaveValue("k3y-2");
  await expect(newKey).toBeFocused();
  await page.getByRole("button", { name: "Copy the key" }).click();
  expect((await copied(page)).at(-1)).toBe("k3y-2");

  await access.click();
  expect((await invoked(page, "set_agent_access")).at(-1)).toEqual({ enabled: false });
  await expect(address).toHaveCount(0);
});

test("a port another app took is explained, with a way out", async ({ page }) => {
  const fixture = signedIn();
  fixture.app.settings.agents = { enabled: true, port: 52731, key: "k3y-1" };
  fixture.app.agents = {
    status: "failed",
    message: "Another app is using port 52731. Move Solidbar to another port, then give your agents the new address.",
  };
  await openSettings(page, fixture);
  await expect(page.getByRole("alert").filter({ hasText: "port 52731" })).toBeVisible();
  await expectAccessible(page);

  await page.getByRole("button", { name: "Use another port" }).click();
  expect(await invoked(page, "move_agent_server")).toHaveLength(1);
  await expect(page.getByLabel("Address", { exact: true })).toHaveValue("http://127.0.0.1:52800/mcp");
});
