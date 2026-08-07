import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  listVaultDir,
  readSidecar,
} from "./helpers";

/**
 * Settings UX — verify manual ordering (order.json) and favorites (favorites.json)
 * all the way from the real UI → backend IPC → the settings written to disk.
 *
 * Favorites are exposed in the tree as a star affordance (read indicator + toggle) and can also be
 * toggled via palette command mode ('>'). Both paths persist to favorites.json.
 *
 * These files are kept outside the notes folder, so each test asserts both halves: the setting was
 * written where settings belong, and the notes folder gained nothing. Asserting only the first
 * would keep passing if the app started leaving files among the notes again.
 */

/** Names the notes folder holds beyond the notes themselves — expected to stay empty. */
function strayEntries(vault: string): string[] {
  return listVaultDir(vault, ".").filter((name) => name !== ".git" && !name.endsWith(".md"));
}

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/** Run a command via palette command mode (query is fuzzy with a '>' prefix). */
async function runCommand(p: Page, query: string): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-input")).toBeVisible();
  await p.getByTestId("palette-input").fill(query);
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

test("manual ordering: 'move down' → visual order swap + order.json persists", async () => {
  const vault = createTempVault({ "alpha.md": "a\n", "bravo.md": "b\n", "charlie.md": "c\n" });
  try {
    await loadVault(page, vault);
    const items = page.getByRole("treeitem");
    await expect(items).toHaveCount(3);
    const before = await items.allTextContents();

    // Select the first item, then run the 'move down' command.
    await items.first().click();
    await runCommand(page, ">move down");

    // The first and second items should be swapped.
    await expect(async () => {
      const after = await items.allTextContents();
      expect(after[0]).toBe(before[1]);
      expect(after[1]).toBe(before[0]);
    }).toPass({ timeout: 5_000 });

    // The new order persists in order.json[root] (root key = vault absolute path, value = array of node paths).
    await expect
      .poll(() => {
        const order = readSidecar(vault, "order.json") as Record<string, string[]> | null;
        if (!order) return null;
        const key = Object.keys(order)[0];
        return order[key]?.[0] ?? null;
      })
      .toContain("bravo"); // before[1] = bravo moves to the front

    // ...and the notes folder is exactly what it was.
    expect(strayEntries(vault)).toEqual([]);
  } finally {
    removeTempVault(vault);
  }
});

test("favorite toggle → favorites.json add/remove persists", async () => {
  const vault = createTempVault({ "pinned-note.md": "x\n" });
  try {
    await loadVault(page, vault);
    const node = page.getByRole("treeitem", { name: /pinned-note/ });

    // Toggle ON → 1 entry in favorites.json.
    await node.click();
    await runCommand(page, ">favorite");
    await expect
      .poll(() => (readSidecar(vault, "favorites.json") as string[] | null)?.length ?? 0)
      .toBe(1);

    // Toggle OFF → favorites.json is emptied.
    await node.click();
    await runCommand(page, ">favorite");
    await expect
      .poll(() => (readSidecar(vault, "favorites.json") as string[] | null)?.length ?? 0)
      .toBe(0);

    expect(strayEntries(vault)).toEqual([]);
  } finally {
    removeTempVault(vault);
  }
});

test("tree star: toggle favorite from the row + reflects state + persists", async () => {
  const vault = createTempVault({ "starme.md": "x\n" });
  try {
    await loadVault(page, vault);
    await expect(page.getByRole("treeitem", { name: /starme/ })).toBeVisible();

    // Star the node from the tree row → state flips to "Remove from favorites" + persists.
    await page.getByRole("button", { name: "Add to favorites" }).click();
    await expect(page.getByRole("button", { name: "Remove from favorites" })).toBeVisible();
    await expect
      .poll(() => (readSidecar(vault, "favorites.json") as string[] | null)?.length ?? 0)
      .toBe(1);

    // Un-star → back to "Add to favorites" + favorites.json emptied.
    await page.getByRole("button", { name: "Remove from favorites" }).click();
    await expect(page.getByRole("button", { name: "Add to favorites" })).toBeVisible();
    await expect
      .poll(() => (readSidecar(vault, "favorites.json") as string[] | null)?.length ?? 0)
      .toBe(0);

    expect(strayEntries(vault)).toEqual([]);
  } finally {
    removeTempVault(vault);
  }
});
