import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * P1b — Body full-text search. Opens the palette with Ctrl+P, queries with the '/'
 * prefix (body search mode), and asserts a body match + snippet via the real
 * tantivy index (app data) and backend IPC.
 *
 * Uses an isolated temp vault (same pattern as other structure/edit specs). Plants
 * a unique token that exists only in the body to ensure the body index matches,
 * not filename fuzzy.
 *
 * Note: open_vault builds the index on a background thread. The index may be empty
 * at the first query, so Playwright's auto-retrying expect(timeout) naturally waits
 * until the build completes.
 */

let browser: Browser;
let page: Page;

// Unique token that appears only in the body (not in filename/title -> only body index matches).
const TOKEN = "전문검색고유토큰";

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("body full-text search: '/' mode body match + snippet", async () => {
  const vault = createTempVault({
    "메모.md": `# 메모\n\n오늘 ${TOKEN} 기능을 설계했다.\n`,
    "딴노트.md": "# 딴노트\n\n관계없는 본문.\n",
  });
  try {
    await loadVault(page, vault);

    // Open palette.
    await page.keyboard.press("Control+p");
    const input = page.getByTestId("palette-input");
    await expect(input).toBeVisible();

    // Switch to body search mode with '/' prefix, then query the unique token.
    await input.fill(`/${TOKEN}`);

    // Index builds in background — auto-retry until results appear.
    const items = page.getByTestId("palette-item");
    await expect(items.first()).toBeVisible({ timeout: 10_000 });

    // Only 1 body match (메모.md), snippet contains the query token.
    await expect(items).toHaveCount(1);
    await expect(items.first()).toContainText(TOKEN);

    // Enter -> palette closes and the note loads in the editor.
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
    await expect(page.locator(".title")).toContainText("메모");
  } finally {
    removeTempVault(vault);
  }
});
