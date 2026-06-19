import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * A2 — flagship command keyboard shortcuts + palette discoverability.
 *
 * The palette shows each command's accelerator (so users can learn it), and the global key
 * handler wires Ctrl+N (new note) / Ctrl+Shift+N (new folder) to enter the inline name editor.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("palette shows the Ctrl+N hint next to New note", async () => {
  await loadVault(page, sampleVaultPath());

  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await page.getByTestId("palette-input").type(">new note");

  // The "New note (root)" command row renders its shortcut hint.
  await expect(page.getByText("Ctrl+N", { exact: true })).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});

test("Ctrl+N opens the new-note name editor", async () => {
  await loadVault(page, sampleVaultPath());

  // Global accelerator → enters new-note mode (inline name input with the "Note name" placeholder).
  await page.keyboard.press("Control+n");
  await expect(page.locator(".name-input")).toBeVisible();
  await expect(page.locator(".name-input")).toHaveAttribute("placeholder", "Note name");

  // Escape (sent to the focused name input) cancels the mode, leaving no dangling editor.
  await page.locator(".name-input").press("Escape");
  await expect(page.locator(".name-input")).toHaveCount(0);
});

test("Ctrl+N is suppressed while the palette is open (no layered editor)", async () => {
  await loadVault(page, sampleVaultPath());

  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  // The accelerator must not fire underneath the open palette: the palette panel's keydown
  // stops propagation, so Ctrl+N never reaches the window-level handler. Guards that mechanism.
  await page.keyboard.press("Control+n");
  await expect(page.locator(".name-input")).toHaveCount(0);
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});
