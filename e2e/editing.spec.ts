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
  const vault = createTempVault({ "memo.md": "initial content\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /memo/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    // Type an extra line at the end of the body.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("appended line");

    // Poll until written to disk after the debounce (500ms) — directly verify the real fs result.
    await expect
      .poll(() => readVaultFile(vault, "memo.md"), { timeout: 5000 })
      .toContain("appended line");
  } finally {
    removeTempVault(vault);
  }
});

test("typing continues across the debounced autosave (no focus loss)", async () => {
  // Regression for the focus-loss bug: after the debounced save, the watcher echoed the app's own
  // atomic write as an external change → editor was recreated → typing focus was dropped, so a
  // second burst never landed. Here we type, WAIT past the save + watcher debounce, then keep
  // typing WITHOUT re-focusing. If focus were lost, the second burst would not reach disk.
  const vault = createTempVault({ "consecutive.md": "start\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /consecutive/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("first");

    // Let the autosave (500ms) AND the watcher debounce (300ms) + any echo fully settle.
    await expect
      .poll(() => readVaultFile(vault, "consecutive.md"), { timeout: 5000 })
      .toContain("first");
    await page.waitForTimeout(900);

    // Keep typing without clicking back into the editor — only possible if focus was retained.
    await page.keyboard.type("second");
    await expect
      .poll(() => readVaultFile(vault, "consecutive.md"), { timeout: 5000 })
      .toContain("second");

    // Both bursts landed contiguously → the caret stayed put; typing was never interrupted.
    expect(readVaultFile(vault, "consecutive.md")).toContain("firstsecond");
  } finally {
    removeTempVault(vault);
  }
});

test("flush unsaved edits when switching notes", async () => {
  const vault = createTempVault({ "note-a.md": "body-a\n", "note-b.md": "body-b\n" });
  try {
    await loadVault(page, vault);

    // Edit note A (switch to note B immediately, before the debounce expires).
    await page.getByRole("treeitem", { name: /note-a/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("XYZ");

    // Switch to note B immediately → flush must persist note A's edit to disk.
    await page.getByRole("treeitem", { name: /note-b/ }).click();

    await expect
      .poll(() => readVaultFile(vault, "note-a.md"), { timeout: 5000 })
      .toContain("XYZ");
  } finally {
    removeTempVault(vault);
  }
});

test("CRLF note round-trips byte-faithfully (Obsidian vault interop)", async () => {
  // A Windows Obsidian vault (often CRLF under git) must not be silently rewritten to LF when
  // edited in Textree — that would dirty every line and break lossless coexistence.
  const vault = createTempVault({ "windows-note.md": "first line\r\nsecond line\r\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /windows-note/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("third line");

    await expect
      .poll(() => readVaultFile(vault, "windows-note.md"), { timeout: 5000 })
      .toContain("third line");

    const out = readVaultFile(vault, "windows-note.md");
    // The pre-existing CRLFs survive and the newly typed break is CRLF too — no lone LF anywhere.
    expect(out.startsWith("first line\r\nsecond line\r\n")).toBe(true);
    expect(/[^\r]\n/.test(out)).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});
