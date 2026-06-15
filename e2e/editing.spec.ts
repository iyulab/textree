import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
} from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("edit → debounced autosave → persisted to disk", async () => {
  const vault = createTempVault({ "메모.md": "처음 내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /메모/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    // Type an extra line at the end of the body.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("추가된 줄");

    // Poll until written to disk after the debounce (500ms) — directly verify the real fs result.
    await expect
      .poll(() => readVaultFile(vault, "메모.md"), { timeout: 5000 })
      .toContain("추가된 줄");
  } finally {
    removeTempVault(vault);
  }
});

test("flush unsaved edits when switching notes", async () => {
  const vault = createTempVault({ "노트가.md": "가본문\n", "노트나.md": "나본문\n" });
  try {
    await loadVault(page, vault);

    // Edit note A (switch to note B immediately, before the debounce expires).
    await page.getByRole("treeitem", { name: /노트가/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("XYZ");

    // Switch to note B immediately → flush must persist note A's edit to disk.
    await page.getByRole("treeitem", { name: /노트나/ }).click();

    await expect
      .poll(() => readVaultFile(vault, "노트가.md"), { timeout: 5000 })
      .toContain("XYZ");
  } finally {
    removeTempVault(vault);
  }
});
