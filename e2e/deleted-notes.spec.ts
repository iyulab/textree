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

test("deleted notes: the open list follows deletions, and does not carry over to another folder", async () => {
  const first = createTempVault({ "one.md": "one\n", "two.md": "two\n" });
  const second = createTempVault({ "other.md": "other\n" });
  try {
    await loadVault(page, first);
    await page.keyboard.press("Control+p");
    await page.getByTestId("palette-input").fill(">deleted");
    await expect(page.getByTestId("palette-item").first()).toContainText("Deleted notes");
    await page.keyboard.press("Enter");
    const panel = page.getByTestId("deleted-notes");
    await expect(panel).toContainText("Nothing has been deleted");

    // Deleted while the list is open: it appears without reopening the list.
    const target = page.getByRole("treeitem", { name: /one/ });
    await target.focus();
    await page.keyboard.press("Delete");
    await expect(panel).toContainText("one");
    await page.getByTestId("deleted-restore").first().click();
    await expect(page.getByTestId("deleted-outcome")).toContainText("Restored");
    // Leave one deleted note listed, so the switch below has something to drop.
    const other = page.getByRole("treeitem", { name: /two/ });
    await other.focus();
    await page.keyboard.press("Delete");
    await expect(panel).toContainText("two");

    // Another folder: the list is that folder's, and what was said about the first one is gone.
    await loadVault(page, second);
    await expect(panel).not.toContainText("two");
    await expect(panel).toContainText("Nothing has been deleted");
    await expect(page.getByTestId("deleted-outcome")).toHaveCount(0);
    await page.getByRole("button", { name: "Close deleted notes" }).click();
  } finally {
    removeTempVault(first);
    removeTempVault(second);
  }
});
