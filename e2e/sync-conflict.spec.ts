import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Sync-conflict surfacing — non-destructive banner.
 *
 * Uses an isolated temp vault so the conflict-named fixture does not pollute sample-vault.
 *
 * Scenario:
 *  1. A vault with a sync-tool conflict copy (Dropbox-style) + a clean note.
 *  2. On open, a non-destructive banner surfaces the conflict copy (the clean note is not flagged).
 *  3. Clicking the conflict entry opens that note (no data changed).
 *  4. Dismiss hides the banner.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("sync-conflict: conflict copy is surfaced non-destructively and is openable", async () => {
  const vault = createTempVault({
    "plan.md": "the real plan\n",
    "plan (John's conflicted copy 2026-06-18).md": "the diverged copy\n",
  });

  try {
    await loadVault(page, vault);

    // The conflict copy is in the tree as an ordinary note (a sync tool left it there).
    await expect(page.getByRole("treeitem", { name: /conflicted copy/ })).toBeVisible();

    // The banner surfaces the conflict copy — and ONLY it (the clean "plan" note is not flagged).
    const banner = page.getByRole("status", { name: "Possible sync conflicts" });
    await expect(banner).toBeVisible();
    await expect(banner.getByRole("listitem")).toHaveCount(1);
    const item = banner.getByRole("button", { name: /conflicted copy/ });
    await expect(item).toBeVisible();

    // Clicking it opens that note (title header reflects the conflict file name).
    await item.click();
    await expect(page.locator(".note-name")).toContainText("conflicted copy");

    // Dismiss hides the banner.
    await banner.getByRole("button", { name: "Dismiss" }).click();
    await expect(page.getByRole("status", { name: "Possible sync conflicts" })).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});
