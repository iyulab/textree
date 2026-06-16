import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * P1 pretty-by-default — frontmatter page header + editor folding.
 * A leading `---` block renders as a page header (title/icon) above the editor, and the raw YAML
 * is folded into a "Properties" pill while the cursor is elsewhere. The source `.md` is unchanged.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

const WITH_FM = "---\ntitle: My Page\nicon: 📓\n---\n# Body heading\ncontent\n";

test("frontmatter renders a page header and folds the source", async () => {
  const vault = createTempVault({ "fm.md": WITH_FM });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /fm/ }).click();

    // Page header shows the title + icon from frontmatter.
    await expect(page.locator(".page-title")).toHaveText("My Page");
    await expect(page.locator(".page-icon")).toHaveText("📓");

    // Raw YAML is folded into a pill by default (the cursor starts in the body, not the block).
    await expect(page.locator(".cm-lp-frontmatter")).toBeVisible();
    await expect(page.locator(".cm-lp-frontmatter")).toContainText("title");

    // Body still renders below the folded block.
    await expect(page.locator(".cm-content")).toContainText("Body heading");
  } finally {
    removeTempVault(vault);
  }
});

test("clicking the pill reveals the raw frontmatter source", async () => {
  const vault = createTempVault({ "fm.md": WITH_FM });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /fm/ }).click();

    await page.locator(".cm-lp-frontmatter").click();
    // Once the cursor is inside the block, the pill is gone and the source line is editable.
    await expect(page.locator(".cm-lp-frontmatter")).toHaveCount(0);
    await expect(page.locator(".cm-editor")).toContainText("title: My Page");
  } finally {
    removeTempVault(vault);
  }
});

test("a note without frontmatter shows no page header", async () => {
  const vault = createTempVault({ "plain.md": "Just a note\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /plain/ }).click();

    await expect(page.locator(".cm-content")).toContainText("Just a note");
    await expect(page.locator(".page-header")).toHaveCount(0);
    await expect(page.locator(".cm-lp-frontmatter")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});
