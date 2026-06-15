import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * D4 — Live preview. Asserts that headings/emphasis/code render inline, and that
 * markers (#, **, `) are hidden on lines without the cursor and reappear when the
 * cursor moves onto that line.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// Line 1 = plain text (default cursor position) -> line 2 heading and line 3 emphasis/code are inactive lines so markers are hidden.
const NOTE = "본문 시작\n# 큰제목\n**굵게** 그리고 `코드` 그리고 ~~삭제~~\n";

test("heading/emphasis/code inline render + marker hidden", async () => {
  const vault = createTempVault({ "lp.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /lp/ }).click();

    // Decoration DOM present.
    await expect(page.locator(".cm-lp-h1")).toBeVisible();
    await expect(page.locator(".cm-lp-strong")).toHaveText("굵게");
    await expect(page.locator(".cm-lp-code")).toHaveText("코드");
    await expect(page.locator(".cm-lp-strike")).toHaveText("삭제");

    // Inactive line: heading marker (#) hidden -> line text is "큰제목" (no #).
    await expect(page.locator(".cm-lp-h1")).toHaveText("큰제목");
  } finally {
    removeTempVault(vault);
  }
});

// Line 1 = plain text (active) -> line 2 onward LP render.
const RICH =
  "본문\n[링크텍스트](https://example.com)\n- [ ] 할일\n> 인용문\n---\n끝\n";

test("link / checkbox / quote / divider render", async () => {
  const vault = createTempVault({ "rich.md": RICH });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /rich/ }).click();

    // Link: only text shown (URL hidden).
    await expect(page.locator(".cm-lp-link")).toHaveText("링크텍스트");

    // Quote/divider decorations.
    await expect(page.locator(".cm-lp-quote")).toBeVisible();
    await expect(page.locator(".cm-lp-hr")).toBeVisible();

    // Checkbox: unchecked -> click -> checked (source [ ]->[x] toggle).
    const box = page.locator(".cm-lp-checkbox");
    await expect(box).not.toBeChecked();
    await box.click();
    await expect(page.locator(".cm-lp-checkbox")).toBeChecked();
  } finally {
    removeTempVault(vault);
  }
});

test("line with cursor exposes markers (source)", async () => {
  const vault = createTempVault({ "lp.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /lp/ }).click();

    // Click heading line -> that line is active -> marker (#) visible again.
    await page.locator(".cm-lp-h1").click();
    await expect(page.locator(".cm-lp-h1")).toContainText("#");
  } finally {
    removeTempVault(vault);
  }
});
