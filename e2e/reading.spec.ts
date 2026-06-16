import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * P1 pretty-by-default — reading view toggle. Switching to reading mode renders a clean read-only
 * view: all markdown markers are hidden (regardless of cursor) and the editor is not editable.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

const NOTE = "# Heading one\n**bold** and `code`\n";

test("reading toggle hides markers and makes the editor read-only", async () => {
  const vault = createTempVault({ "read.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /read/ }).click();

    // Edit mode: line 1 is the default active line, so its heading marker (#) is visible, and the
    // content is editable.
    await expect(page.locator(".cm-lp-h1")).toContainText("#");
    await expect(page.locator(".cm-content")).toHaveAttribute("contenteditable", "true");

    // Toggle to reading view.
    await page.getByRole("button", { name: "Switch to reading view" }).click();

    // Reading mode: marker hidden everywhere (no active line) and content read-only.
    await expect(page.locator(".cm-lp-h1")).toHaveText("Heading one");
    await expect(page.locator(".cm-content")).toHaveAttribute("contenteditable", "false");

    // Toggle back to editing.
    await page.getByRole("button", { name: "Switch to editing" }).click();
    await expect(page.locator(".cm-content")).toHaveAttribute("contenteditable", "true");
  } finally {
    removeTempVault(vault);
  }
});
