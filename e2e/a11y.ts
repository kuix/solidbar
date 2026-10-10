import AxeBuilder from "@axe-core/playwright";
import { expect, type Page } from "@playwright/test";

/** Runs axe-core against WCAG 2.2 A/AA and fails with the rule ids and elements. */
export async function expectAccessible(page: Page) {
  // Contrast measured halfway through a fade-in (the toast) is meaningless; let finite
  // animations settle first. The running timer's pulse repeats forever, so skip those.
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.effect?.getTiming().iterations === Infinity || animation.playState !== "running"),
  );
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const violations = results.violations.map(
    (violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
  );
  expect(violations).toEqual([]);
}
