import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Editor KaTeX math rendering. Inline `$..$` and display `$$..$$` render as KaTeX; the cursor
 * reveals the raw source; reading mode renders both. Also asserts the woff2 fonts actually load
 * (the classic KaTeX integration trap: CSS present but fonts 404 -> silent fallback font).
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// Line 1 = plain (default cursor) so lines below are inactive and render.
const NOTE = "body start\ninline $E=mc^2$ here\n\n$$\n\\int_0^1 x\\,dx\n$$\n";

test("inline math renders as KaTeX on an inactive line", async () => {
  const vault = createTempVault({ "math.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /math/ }).click();
    const inline = page.locator(".cm-lp-math-inline .katex");
    await expect(inline).toBeVisible();
    // The raw `$` delimiters are gone from the rendered inline widget.
    await expect(page.locator(".cm-lp-math-inline")).not.toContainText("$");
  } finally {
    removeTempVault(vault);
  }
});

test("display math renders as a centered KaTeX block", async () => {
  const vault = createTempVault({ "math.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /math/ }).click();
    await expect(page.locator(".cm-lp-math-block .katex-display")).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("cursor on the inline line reveals raw $..$ source", async () => {
  const vault = createTempVault({ "math.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /math/ }).click();
    await expect(page.locator(".cm-lp-math-inline .katex")).toBeVisible();
    // Click the rendered inline math -> its line becomes active -> raw `$E=mc^2$` returns.
    await page.locator(".cm-lp-math-inline").click();
    const line = page.locator(".cm-content .cm-line", { hasText: "inline" });
    await expect(line).toContainText("$E=mc^2$");
    expect(await line.locator(".cm-lp-math-inline").count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});

test("katex woff2 fonts actually load (no silent fallback)", async () => {
  const vault = createTempVault({ "math.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /math/ }).click();
    await expect(page.locator(".katex").first()).toBeVisible();
    // At least one KaTeX_* font face must be loaded (document.fonts), else glyphs fell back.
    const loaded = await page.evaluate(async () => {
      await (document as unknown as { fonts: { ready: Promise<unknown> } }).fonts.ready;
      return [...(document as unknown as { fonts: Set<{ family: string; status: string }> }).fonts]
        .some((f) => f.family.startsWith("KaTeX") && f.status === "loaded");
    });
    expect(loaded).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

// Regression: currency text must not be swallowed as math.
const CURRENCY = "line1\nI paid $5 and $10 today\n";

test("currency is not rendered as math (no false positive)", async () => {
  const vault = createTempVault({ "cur.md": CURRENCY });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /cur/ }).click();
    const line = page.locator(".cm-content .cm-line", { hasText: "I paid" });
    await expect(line).toContainText("$5 and $10");
    expect(await page.locator(".cm-lp-math-inline").count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});

// Regression: a wikilink whose inner text contains inline math must render as a wikilink (wiki wins),
// with no overlapping math widget and no editor crash. Guards the wiki-vs-math precedence fix.
const WIKI_MATH = "line1\nsee [[$x$]] here\n";

test("wikilink containing $..$ renders as a wikilink, not math (no overlap/crash)", async () => {
  const vault = createTempVault({ "wm.md": WIKI_MATH });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /wm/ }).click();
    // The wikilink widget renders (label is the raw target text since the note is unresolved)...
    await expect(page.locator(".cm-lp-wikilink")).toBeVisible();
    // ...and no inline-math widget was produced for the `$x$` inside it.
    expect(await page.locator(".cm-lp-math-inline").count()).toBe(0);
    // Editor still functional (content rendered, no RangeSet crash blanking the view).
    await expect(page.locator(".cm-content")).toContainText("here");
  } finally {
    removeTempVault(vault);
  }
});
