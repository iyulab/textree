import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
} from "./helpers";

/**
 * Frontmatter table (folder = DB, .md = row) — read-only first slice.
 *
 * Selecting a folder shows a read-only table of its direct child notes: frontmatter keys become
 * columns (unioned), one row per note. Clicking a row opens that note.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("folder table: selecting a folder shows its notes' frontmatter as a table", async () => {
  const vault = createTempVault({
    "projects/alpha.md": "---\nstatus: done\nowner: me\n---\n# Alpha\n",
    "projects/beta.md": "---\nstatus: wip\n---\n# Beta\n",
    "loose.md": "# Loose note\n",
  });

  try {
    await loadVault(page, vault);

    // Open an unrelated root note first.
    await page.getByRole("treeitem", { name: /loose/ }).click();
    await expect(page.locator(".note-name")).toContainText("loose");

    // Select the (body-less) folder. The stale note view is cleared — only the folder's table shows.
    const folder = page.getByRole("treeitem", { name: /projects/ });
    await expect(folder).toBeVisible();
    await folder.click();
    await expect(page.locator(".note-name")).toHaveCount(0);

    // The folder table appears with the unioned frontmatter columns.
    const table = page.getByRole("region", { name: "Folder table" });
    await expect(table).toBeVisible();
    await expect(table.getByRole("columnheader", { name: "status" })).toBeVisible();
    await expect(table.getByRole("columnheader", { name: "owner" })).toBeVisible();

    // One row per child note (both notes present); the loose note at root is not included.
    await expect(table.getByRole("button", { name: "alpha" })).toBeVisible();
    await expect(table.getByRole("button", { name: "beta" })).toBeVisible();
    await expect(table.getByRole("button", { name: "loose" })).toHaveCount(0);

    // A cell shows the frontmatter value.
    await expect(table.getByRole("cell", { name: "done", exact: true })).toBeVisible();

    // Sort by the "status" column: asc → done(alpha) before wip(beta).
    await table.getByRole("button", { name: "status" }).click();
    await expect(table.locator("tbody .row-open")).toHaveText(["alpha", "beta"]);
    await expect(table.getByRole("columnheader", { name: "status" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
    // Clicking again toggles to desc → wip(beta) before done(alpha).
    await table.getByRole("button", { name: "status" }).click();
    await expect(table.locator("tbody .row-open")).toHaveText(["beta", "alpha"]);

    // Clicking a row opens that note.
    await table.getByRole("button", { name: "alpha" }).click();
    await expect(page.locator(".note-name")).toContainText("alpha");
  } finally {
    removeTempVault(vault);
  }
});

test("folder table: filter, save a named view, persist, reselect, delete", async () => {
  const vault = createTempVault({
    "tasks/alpha.md": "---\nstatus: done\n---\n# Alpha\n",
    "tasks/beta.md": "---\nstatus: wip\n---\n# Beta\n",
    "tasks/gamma.md": "---\nstatus: done\n---\n# Gamma\n",
  });

  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /tasks/ }).click();

    const table = page.getByRole("region", { name: "Folder table" });
    await expect(table).toBeVisible();
    await expect(table.locator("tbody .row-open")).toHaveText(["alpha", "beta", "gamma"]);

    // Add a filter: status contains "done" → alpha, gamma (beta is wip).
    await table.getByRole("button", { name: "+ Filter" }).click();
    await table.getByRole("textbox", { name: "Filter value" }).fill("done");
    await expect(table.locator("tbody .row-open")).toHaveText(["alpha", "gamma"]);

    // Save the current view as "Done".
    await table.getByRole("button", { name: "Save view" }).click();
    await table.getByRole("textbox", { name: "View name" }).fill("Done");
    await table.getByRole("button", { name: "Save", exact: true }).click();

    // It persists to .textree/views.json on disk (folder-keyed; new IPC 0, reuses write_sidecar).
    await expect
      .poll(() => {
        try {
          return readVaultFile(vault, ".textree/views.json");
        } catch {
          return "";
        }
      })
      .toContain("Done");

    // "All" returns to the unfiltered view.
    await table.getByRole("button", { name: "All", exact: true }).click();
    await expect(table.locator("tbody .row-open")).toHaveText(["alpha", "beta", "gamma"]);

    // Reload the vault → the saved view loads from disk and re-applies on click.
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /tasks/ }).click();
    await table.getByRole("button", { name: "Done", exact: true }).click();
    await expect(table.locator("tbody .row-open")).toHaveText(["alpha", "gamma"]);

    // Delete the view → its chip disappears.
    await table.getByRole("button", { name: "Delete view Done" }).click();
    await expect(table.getByRole("button", { name: "Done", exact: true })).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});
