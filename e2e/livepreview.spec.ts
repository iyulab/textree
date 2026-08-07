import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Live preview. Asserts that headings/emphasis/code render inline, and that
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
const NOTE = "body start\n# Big title\n**bold** and `code` and ~~deleted~~\n";

test("heading/emphasis/code inline render + marker hidden", async () => {
  const vault = createTempVault({ "lp.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /lp/ }).click();

    // Decoration DOM present.
    await expect(page.locator(".cm-lp-h1")).toBeVisible();
    await expect(page.locator(".cm-lp-strong")).toHaveText("bold");
    await expect(page.locator(".cm-lp-code")).toHaveText("code");
    await expect(page.locator(".cm-lp-strike")).toHaveText("deleted");

    // Inactive line: heading marker (#) hidden -> line text is "Big title" (no #).
    await expect(page.locator(".cm-lp-h1")).toHaveText("Big title");
  } finally {
    removeTempVault(vault);
  }
});

// Line 1 = plain text (active) -> line 2 onward LP render.
const RICH =
  "body\n[link-text](https://example.com)\n- [ ] todo\n> quotation\n---\nend\n";

test("link / checkbox / quote / divider render", async () => {
  const vault = createTempVault({ "rich.md": RICH });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /rich/ }).click();

    // Link: only text shown (URL hidden).
    await expect(page.locator(".cm-lp-link")).toHaveText("link-text");

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

// A ```js fence lazy-loads its grammar; highlighted tokens carry >1 distinct color.
const CODE = "body\n```js\nconst x = 42; // hi\n```\n";

test("fenced code block gets language syntax highlighting", async () => {
  const vault = createTempVault({ "code.md": CODE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /code/ }).click();
    // The js grammar imports asynchronously on first use; poll until the code line
    // shows more than one token color (proof of highlighting), robust to CM class churn.
    await expect
      .poll(
        async () =>
          page.evaluate(() => {
            const line = [...document.querySelectorAll(".cm-content .cm-line")].find((l) =>
              l.textContent?.includes("const x = 42"),
            );
            if (!line) return 0;
            const colors = new Set(
              [...line.querySelectorAll("span")].map((s) => getComputedStyle(s).color),
            );
            return colors.size;
          }),
        { timeout: 5000 },
      )
      .toBeGreaterThan(1);
  } finally {
    removeTempVault(vault);
  }
});

// A fence with no language has no nested grammar -> no per-token colors (fallback).
const PLAIN_FENCE = "body\n```\nconst x = 42;\n```\n";

test("language-less fence is not syntax-highlighted (fallback)", async () => {
  const vault = createTempVault({ "plain.md": PLAIN_FENCE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /plain/ }).click();
    const codeLine = page.locator(".cm-content .cm-line", { hasText: "const x = 42" });
    await expect(codeLine).toBeVisible();
    const colorCount = await page.evaluate(() => {
      const line = [...document.querySelectorAll(".cm-content .cm-line")].find((l) =>
        l.textContent?.includes("const x = 42"),
      );
      if (!line) return 0;
      return new Set([...line.querySelectorAll("span")].map((s) => getComputedStyle(s).color)).size;
    });
    expect(colorCount).toBeLessThanOrEqual(1);
  } finally {
    removeTempVault(vault);
  }
});

// Regression: a ```md fence must NOT get live-preview decorations (markers stay raw, no bold styling).
// Guards the IterMode.IgnoreMounts fix — nested markdown grammar mounts must not leak into the editor.
const MD_FENCE = "body\n```md\n**bold** and # heading\n```\n";

test("markdown fence keeps raw markers (no live-preview leak)", async () => {
  const vault = createTempVault({ "mdfence.md": MD_FENCE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /mdfence/ }).click();
    const codeLine = page.locator(".cm-content .cm-line", { hasText: "bold" });
    await expect(codeLine).toBeVisible();
    // Inside the fence the markers stay literal (not hidden as they would be in prose)...
    await expect(codeLine).toContainText("**bold**");
    await expect(codeLine).toContainText("# heading");
    // ...and no live-preview bold decoration leaks in.
    expect(await codeLine.locator(".cm-lp-strong").count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});
