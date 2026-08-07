import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

/**
 * Delete → find it under Deleted notes → bring it back.
 *
 * Replaces the round trip that used to go through a folder of set-aside copies. The copies are
 * gone; the same journey now goes through the history, and the folder is expected to be left
 * holding nothing but notes and the repository the history lives in.
 *
 * An isolated temp vault, not the shared fixture: deleting is destructive.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("deleted notes: delete a note → it is listed → restoring returns it to the tree", async () => {
  const vault = createTempVault({
    "doomed-note.md": "content to delete\n",
    "keep-note.md": "keep this\n",
  });

  try {
    await loadVault(page, vault);

    const target = page.getByRole("treeitem", { name: /doomed-note/ });
    await expect(target).toBeVisible();
    await target.focus();
    await page.keyboard.press("Delete");

    await expect(page.getByRole("treeitem", { name: /doomed-note/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /keep-note/ })).toBeVisible();

    // Nothing was set aside inside the folder — the note is gone from it entirely.
    expect(existsSync(join(vault, ".textree"))).toBe(false);
    expect(readdirSync(vault).sort()).toEqual([".git", "keep-note.md"]);

    await page.keyboard.press("Control+p");
    await expect(page.getByTestId("palette-overlay")).toBeVisible();
    await page.getByTestId("palette-input").type(">deleted");
    await expect(page.getByTestId("palette-item").first()).toBeVisible();
    await page.keyboard.press("Enter");

    await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
    const panel = page.getByTestId("deleted-notes");
    await expect(panel).toBeVisible();
    await expect(panel).toContainText("doomed-note");

    await page.getByTestId("deleted-restore").first().click();

    // It says which state came back, and the note is in the tree again.
    await expect(page.getByTestId("deleted-outcome")).toContainText("Restored");
    await expect(page.getByRole("treeitem", { name: /doomed-note/ })).toBeVisible();
    await expect(page.getByRole("treeitem", { name: /keep-note/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});
