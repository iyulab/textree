import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * ⋮ app menu — categorized command dropdown sharing the palette's command source.
 * Drives the real WebView2 (not a mock). sample-vault has an open vault so the
 * ungated View/Vault categories are always present.
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

test("app menu: ⋮ opens a categorized command menu, Esc closes it", async () => {
  await loadVault(page, sampleVaultPath());

  await page.getByTestId("app-menu-button").click();
  await expect(page.getByTestId("app-menu")).toBeVisible();

  const cats = page.getByTestId("app-menu-category");
  await expect(cats.filter({ hasText: "Vault" })).toBeVisible();
  await expect(cats.filter({ hasText: "View" })).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("app-menu")).toHaveCount(0);
});

test("app menu: clicking Toggle theme runs the command and closes the menu", async () => {
  await loadVault(page, sampleVaultPath());

  const html = page.locator("html");
  const before = (await html.getAttribute("data-theme")) ?? "light";

  await page.getByTestId("app-menu-button").click();
  await expect(page.getByTestId("app-menu")).toBeVisible();

  await page.getByTestId("app-menu-item").filter({ hasText: "Toggle theme" }).click();

  await expect(page.getByTestId("app-menu")).toHaveCount(0);
  await expect(html).toHaveAttribute("data-theme", opposite(before));
});

test("app menu: Search all… opens the palette", async () => {
  await loadVault(page, sampleVaultPath());

  await page.getByTestId("app-menu-button").click();
  await page.getByTestId("app-menu-search-all").click();

  await expect(page.getByTestId("app-menu")).toHaveCount(0);
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});
