import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * B1 — trash delete→restore round trip.
 *
 * Uses an isolated temp vault (not sample-vault) so the destructive delete/restore
 * operations do not pollute the shared fixture used by smoke/tree specs.
 *
 * Scenario:
 *  1. Select a leaf note in the tree → press Delete → disappears from tree.
 *  2. Open palette (Ctrl+P) → command mode (>trash) → Enter → trash-panel visible
 *     + trash-item for that note exists.
 *  3. Click trash-restore → panel updates (trash-item count → 0) AND note reappears
 *     in the tree (refreshTree convergence).
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("trash: delete note → appears in trash panel → restore returns it to tree", async () => {
  // Isolated vault: two notes so we can assert the other is unaffected.
  const vault = createTempVault({
    "지울노트.md": "content to delete\n",
    "남길노트.md": "keep this\n",
  });

  try {
    await loadVault(page, vault);

    // ── Step 1: Select the note and Delete it. ──────────────────────────────
    const target = page.getByRole("treeitem", { name: /지울노트/ });
    await expect(target).toBeVisible();
    await target.focus();
    await page.keyboard.press("Delete");

    // The deleted note disappears from the tree; the other note stays.
    await expect(page.getByRole("treeitem", { name: /지울노트/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /남길노트/ })).toBeVisible();

    // ── Step 2: Open palette → command mode → run "Trash…" ─────────────────
    await page.keyboard.press("Control+p");
    await expect(page.getByTestId("palette-overlay")).toBeVisible();

    // ">trash" fuzzy-matches the "Trash…" command.
    await page.getByTestId("palette-input").type(">trash");
    await expect(page.getByTestId("palette-item").first()).toBeVisible();
    await page.keyboard.press("Enter");

    // Palette closes and the trash panel opens.
    await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
    await expect(page.getByTestId("trash-panel")).toBeVisible();

    // The deleted note has a trash-item entry.
    const trashItems = page.getByTestId("trash-item");
    await expect(trashItems).toHaveCount(1);
    // The item's visible name contains the note's stem.
    await expect(trashItems.first()).toContainText("지울노트");

    // ── Step 3: Restore → panel updates + note reappears in tree ───────────
    await page.getByTestId("trash-restore").click();

    // The trash panel is now empty (the item was the only one).
    await expect(page.getByTestId("trash-item")).toHaveCount(0);

    // The tree reflects the restored note (refreshTree convergence).
    await expect(page.getByRole("treeitem", { name: /지울노트/ })).toBeVisible();
    // Sanity: the sibling is still present.
    await expect(page.getByRole("treeitem", { name: /남길노트/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});
