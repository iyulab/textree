import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * The "Not backed up" indicator stays on screen whenever a note is open.
 *
 * It is a standing fact, not a hover affordance: the note's title tools fade in on hover, and
 * the indicator must not be one of them. Playwright's `toBeVisible` counts `opacity: 0` as
 * visible, so the effective opacity is checked directly.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("backup indicator: shown on an open note without hovering, with an explanation", async () => {
  const vault = createTempVault({ "memo.md": "# memo\n\nbody\n" });

  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /memo/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    // Keep the pointer off the title row so hover-only tools stay hidden.
    await page.locator(".cm-content").click();
    await page.mouse.move(0, 0);

    const indicator = page.getByTestId("backup-status");
    await expect(indicator).toHaveText("Not backed up");
    await expect(indicator).toHaveAttribute("title", /only on this computer/);

    const effectiveOpacity = await indicator.evaluate((el) => {
      let opacity = 1;
      for (let node: Element | null = el; node; node = node.parentElement) {
        opacity *= Number(getComputedStyle(node).opacity);
      }
      return opacity;
    });
    expect(effectiveOpacity).toBeGreaterThan(0.5);
  } finally {
    removeTempVault(vault);
  }
});
