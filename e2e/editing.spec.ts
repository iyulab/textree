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

test("typing continues across the debounced autosave (no focus loss)", async () => {
  // Regression for the focus-loss bug: after the debounced save, the watcher echoed the app's own
  // atomic write as an external change → editor was recreated → typing focus was dropped, so a
  // second burst never landed. Here we type, WAIT past the save + watcher debounce, then keep
  // typing WITHOUT re-focusing. If focus were lost, the second burst would not reach disk.
  const vault = createTempVault({ "연속.md": "시작\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /연속/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("첫번째");

    // Let the autosave (500ms) AND the watcher debounce (300ms) + any echo fully settle.
    await expect
      .poll(() => readVaultFile(vault, "연속.md"), { timeout: 5000 })
      .toContain("첫번째");
    await page.waitForTimeout(900);

    // Keep typing without clicking back into the editor — only possible if focus was retained.
    await page.keyboard.type("두번째");
    await expect
      .poll(() => readVaultFile(vault, "연속.md"), { timeout: 5000 })
      .toContain("두번째");

    // Both bursts landed contiguously → the caret stayed put; typing was never interrupted.
    expect(readVaultFile(vault, "연속.md")).toContain("첫번째두번째");
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

test("CRLF note round-trips byte-faithfully (Obsidian vault interop)", async () => {
  // A Windows Obsidian vault (often CRLF under git) must not be silently rewritten to LF when
  // edited in Textree — that would dirty every line and break lossless coexistence.
  const vault = createTempVault({ "윈도우노트.md": "첫 줄\r\n둘째 줄\r\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /윈도우노트/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("셋째 줄");

    await expect
      .poll(() => readVaultFile(vault, "윈도우노트.md"), { timeout: 5000 })
      .toContain("셋째 줄");

    const out = readVaultFile(vault, "윈도우노트.md");
    // The pre-existing CRLFs survive and the newly typed break is CRLF too — no lone LF anywhere.
    expect(out.startsWith("첫 줄\r\n둘째 줄\r\n")).toBe(true);
    expect(/[^\r]\n/.test(out)).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});
