import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath, createTempVault, removeTempVault } from "./helpers";

/**
 * A2 — flagship command keyboard shortcuts + palette discoverability.
 *
 * The palette shows each command's accelerator (so users can learn it). The global key handler
 * wires Ctrl+N (new note → instant Untitled, header title focused) / Ctrl+Shift+N (new folder dialog).
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

test("Ctrl+N creates an Untitled note and focuses the header title (no dialog)", async () => {
  // Isolated temp vault: a fresh vault guarantees the first new note is exactly "Untitled"
  // (sample-vault would accumulate Untitled litter across runs → "Untitled (1)").
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);

    await page.keyboard.press("Control+n");
    // No inline name dialog — header title input is focused, pre-filled "Untitled".
    await expect(page.locator(".name-input")).toHaveCount(0);
    await expect(page.locator(".title-input")).toBeFocused();
    await expect(page.locator(".title-input")).toHaveValue("Untitled");

    // Escape cancels the rename, leaving the note named "Untitled".
    await page.locator(".title-input").press("Escape");
    await expect(page.getByRole("treeitem", { name: /^Untitled$/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("Ctrl+N is suppressed while typing in a form input (no clobbering)", async () => {
  await loadVault(page, sampleVaultPath());

  // Enter new-FOLDER mode (still a dialog) and start typing a name.
  await page.keyboard.press("Control+Shift+n");
  await expect(page.locator(".name-input")).toBeVisible();
  await page.locator(".name-input").fill("진행중");

  // Ctrl+N while focused in the form input must NOT trigger a new note.
  await page.keyboard.press("Control+n");
  await expect(page.locator(".name-input")).toHaveValue("진행중");
  await expect(page.locator(".name-input")).toBeVisible();

  await page.locator(".name-input").press("Escape");
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
