import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * P1a — unified palette. Open with Ctrl+P and assert file search (file mode) and
 * command execution (command mode) against a real WebView2.
 *
 * File mode: default, when the query does not start with '>'.
 * Command mode: when the query starts with '>'. The actual search term is after '>'.
 *
 * sample-vault top-level file: project (leaf).
 * Command title: "Toggle theme (light/dark)" → fuzzy matched by '>theme'.
 */

let browser: Browser;
let page: Page;

const opposite = (t: string) => (t === "dark" ? "light" : "dark");

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("palette file mode: Ctrl+P → file search → Enter → note loads", async () => {
  await loadVault(page, sampleVaultPath());

  // Open the palette.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await expect(page.getByTestId("palette-input")).toBeVisible();

  // Type part of a file name — the real file "project" in sample-vault.
  await page.getByTestId("palette-input").type("project");

  // At least one matching result is shown.
  await expect(page.getByTestId("palette-item").first()).toBeVisible();

  // Enter → palette closes and the note loads in the editor.
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

  // Reuse the editor verification selectors from smoke.spec.ts.
  await expect(page.locator(".title")).toContainText("project");
  await expect(page.locator(".cm-content")).toBeVisible();
});

test("palette Ctrl+P moves focus into the search input even when the editor was focused", async () => {
  await loadVault(page, sampleVaultPath());

  // Faithful to real use: focus is in the note editor when the user hits Ctrl+P.
  // Open a note, then focus the CodeMirror editable so it holds focus before the palette opens.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-input")).toBeVisible();
  await page.getByTestId("palette-input").fill("project");
  await expect(page.getByTestId("palette-item").first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
  await page.locator(".cm-content").click(); // focus the editor (contenteditable)
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.className ?? ""))
    .toContain("cm-content");

  // Now the real scenario: Ctrl+P from the focused editor must move focus to the palette input.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await expect(page.getByTestId("palette-input")).toBeVisible();

  const focusedTestId = await page.evaluate(() =>
    document.activeElement?.getAttribute("data-testid"),
  );
  expect(focusedTestId).toBe("palette-input");

  // Drive keys globally (no locator focus) — reaches the input only if focus actually moved.
  await page.keyboard.type("project");
  await expect(page.getByTestId("palette-item").first()).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});

test("palette command mode: Ctrl+P → type '>theme' → Enter → theme toggles", async () => {
  await loadVault(page, sampleVaultPath());

  const html = page.locator("html");
  const before = (await html.getAttribute("data-theme")) ?? "light";

  // Open the palette.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  // Switch to command mode with the '>' prefix, then type 'theme' — fuzzy matches "Toggle theme (light/dark)".
  await page.getByTestId("palette-input").type(">theme");

  // Command result is shown.
  await expect(page.getByTestId("palette-item").first()).toBeVisible();

  // Enter → palette closes and the theme toggles.
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

  // Reuse the verification approach from theme.spec.ts: the data-theme attribute switches to the opposite value.
  await expect(html).toHaveAttribute("data-theme", opposite(before));
});

test("palette no-results: a query with no matches shows a 'No matches found' status row", async () => {
  await loadVault(page, sampleVaultPath());

  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  // A query that matches no file — distinguishes a settled "no matches" from a silent empty list.
  await page.getByTestId("palette-input").type("zzzznomatchqqq");

  await expect(page.getByTestId("palette-empty")).toBeVisible();
  // The status row is NOT counted as a selectable item (separate testid → no false matches).
  await expect(page.getByTestId("palette-item")).toHaveCount(0);

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});

test("palette Esc: Esc while open → overlay closes", async () => {
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});
