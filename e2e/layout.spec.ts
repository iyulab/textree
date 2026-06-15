import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * D2 — app shell. Assert in the real webview that sidebar collapse/expand +
 * resize work and that the selection persists in localStorage.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/** Normalize the sidebar to the expanded (default) state. */
async function ensureExpanded(page: Page): Promise<void> {
  if (await page.locator(".expand-btn").isVisible().catch(() => false)) {
    await page.locator(".expand-btn").click();
  }
  await expect(page.locator(".sidebar")).toBeVisible();
}

test("sidebar collapse/expand + persist", async () => {
  await ensureExpanded(page);

  // Collapse
  await page.getByRole("button", { name: "사이드바 접기" }).click();
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await expect(page.locator(".expand-btn")).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("true");

  // Expand
  await page.locator(".expand-btn").click();
  await expect(page.locator(".sidebar")).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("false");
});

test("sidebar resize + width persist", async () => {
  // Idempotency: reset the starting width to a narrow baseline (220) before measuring.
  // Otherwise the width persisted by the previous run accumulates and, once it hits the
  // max (480), it can't widen further and the test fails.
  await page.evaluate(() => localStorage.setItem("textree-sidebar-width", "220"));
  await page.reload();
  await page.waitForFunction(() => (window as { __textreeTest?: unknown }).__textreeTest);
  await ensureExpanded(page);

  const sidebar = page.locator(".sidebar");
  const startW = (await sidebar.boundingBox())!.width;

  // Drag the handle to the right to widen (target width below the max of 480).
  const handle = page.locator(".resize-handle");
  const hb = (await handle.boundingBox())!;
  const y = hb.y + hb.height / 2;
  await page.mouse.move(hb.x + hb.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(startW + 120, y, { steps: 8 });
  await page.mouse.up();

  const endW = (await sidebar.boundingBox())!.width;
  expect(endW).toBeGreaterThan(startW + 40);

  // Width persists in localStorage (once at drag end). The stored value exactly matches
  // layout.width (=clientX), but boundingBox (endW) measurement can differ by the scrollbar
  // gutter, so compare with a tolerance on the order of the scrollbar width (the app's
  // actual restore has no drift).
  const stored = Number(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-width")),
  );
  expect(stored).toBeGreaterThan(startW + 40);
  expect(Math.abs(stored - endW)).toBeLessThan(20);
});
