import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * D2 — app shell. Assert in the real webview that sidebar collapse/expand +
 * resize work and that the selection persists in localStorage.
 *
 * The sidebar collapse toggle now lives in the custom title bar (TitleBar.svelte),
 * not in the sidebar itself — the old in-content "Expand sidebar" / sidebar-head
 * "Collapse sidebar" buttons were removed when the title bar shipped. The toggle
 * is a single button whose accessible name flips between "Hide sidebar" (sidebar
 * expanded) and "Show sidebar" (sidebar collapsed).
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
  const showBtn = page.getByRole("button", { name: "Show sidebar" });
  if (await showBtn.isVisible().catch(() => false)) {
    await showBtn.click();
  }
  await expect(page.locator(".sidebar")).toBeVisible();
}

test("sidebar collapse/expand + persist", async () => {
  await ensureExpanded(page);

  // Collapse via the title bar toggle (aria-label flips to "Show sidebar" once collapsed).
  await page.getByRole("button", { name: "Hide sidebar" }).click();
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Show sidebar" })).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("true");

  // Expand
  await page.getByRole("button", { name: "Show sidebar" }).click();
  await expect(page.locator(".sidebar")).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("false");
});

test("title bar toggle collapses and expands the sidebar", async () => {
  await ensureExpanded(page);

  await expect(page.locator(".sidebar")).toBeVisible();
  await page.getByRole("button", { name: /Hide sidebar/i }).click();
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await page.getByRole("button", { name: /Show sidebar/i }).click();
  await expect(page.locator(".sidebar")).toBeVisible();
});

test("title bar center button opens the command palette", async () => {
  await ensureExpanded(page);

  // Not a role-name lookup: the button's accessible name comes from its visible text content
  // (the current note/vault label, e.g. "Textree"), not its `title` attribute — the icon inside
  // is aria-hidden. `.tb-center` is the stable hook (see TitleBar.svelte).
  await page.locator(".tb-center").click();
  await expect(page.locator('[data-testid="palette-overlay"]')).toBeVisible({ timeout: 3_000 });

  // Clean up so later tests in this file don't inherit the palette open. Escape only closes
  // the palette when its input has focus (Palette.svelte binds onKey to the input, not the
  // page); clicking the title bar button leaves focus on that button, so re-focus the input
  // first (same pattern as e2e/semantic.spec.ts's dismissPaletteIfOpen).
  await page.getByTestId("palette-input").focus();
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-testid="palette-overlay"]')).toHaveCount(0);
});

test("sidebar controls are always visible (no hover needed)", async () => {
  await ensureExpanded(page);

  // The theme/settings icons were absorbed into the ⋮ app menu, so the always-visible
  // header controls are now the ⋮ menu button and the vault-name button.
  const menuBtn = page.getByTestId("app-menu-button");
  // Move the mouse away from the sidebar so we're testing the at-rest state, not a hover.
  await page.mouse.move(0, 0);

  await expect(menuBtn).toHaveCSS("opacity", "1");
  await expect(menuBtn).toBeVisible();
  await expect(page.locator(".vault-name")).toBeVisible();
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
