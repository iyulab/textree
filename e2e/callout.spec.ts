import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Callout live rendering (`> [!type]`). On inactive lines the quote renders with
 * type-colored line decorations and the `[!type]` marker is replaced by an icon
 * widget (plus the fallback title when none was written). The cursor entering the
 * head line reveals the raw marker. Recognition semantics are pinned in
 * callout-parity.golden.json; this suite covers the decoration path itself.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// Line 1 = plain (default cursor) so the callout below is inactive and renders.
const NOTE = "body start\n\n> [!note]\n> plain body\n";

test("a bare [!note] renders the icon widget and fallback title", async () => {
  const vault = createTempVault({ "callout.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    const head = page.locator(".cm-line.cm-lp-callout-head-note");
    await expect(head).toBeVisible();
    await expect(head.locator(".cm-lp-callout-icon")).toBeVisible();
    await expect(head).toContainText("Note");
    // The raw marker is consumed by the widget on inactive lines.
    expect(await page.locator(".cm-content .cm-line", { hasText: "[!note]" }).count()).toBe(0);
    // Every callout line carries the tinted line decoration.
    expect(await page.locator(".cm-line.cm-lp-callout.cm-lp-callout-note").count()).toBe(2);
  } finally {
    removeTempVault(vault);
  }
});

test("an explicit title keeps its source text and gets an icon-only widget", async () => {
  const vault = createTempVault({ "callout.md": "body start\n\n> [!tip] Pro move\n> body\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    const head = page.locator(".cm-line.cm-lp-callout-head-tip");
    await expect(head.locator(".cm-lp-callout-icon")).toBeVisible();
    await expect(head).toContainText("Pro move");
    expect(await page.locator(".cm-content .cm-line", { hasText: "[!tip]" }).count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});

test("clicking the head line reveals the raw marker", async () => {
  const vault = createTempVault({ "callout.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    await page.locator(".cm-line.cm-lp-callout-head-note").click();
    await expect(page.locator(".cm-content .cm-line", { hasText: "[!note]" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("aliases map to a core style but title the typed word", async () => {
  const vault = createTempVault({ "callout.md": "body start\n\n> [!info]\n> body\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    const head = page.locator(".cm-line.cm-lp-callout-head-note");
    await expect(head).toBeVisible();
    await expect(head).toContainText("Info");
  } finally {
    removeTempVault(vault);
  }
});

test("plain blockquotes and nested quotes keep the plain quote style", async () => {
  const vault = createTempVault({
    "callout.md": "body start\n\n> just a quote\n\n> [!note]\n> outer\n> > [!tip] Inner\n",
  });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    const plain = page.locator(".cm-line.cm-lp-quote", { hasText: "just a quote" });
    await expect(plain).toBeVisible();
    await expect(plain).not.toHaveClass(/cm-lp-callout/);
    // The nested quote is not a callout: no second icon widget, and its line keeps the
    // plain quote class stacked on the outer callout's tint. Its `[!tip]` text gets the
    // editor's generic `[text]` marker-hiding on inactive lines (pre-existing behavior),
    // so assert on the visible remainder rather than the raw brackets.
    const nested = page.locator(".cm-line.cm-lp-quote.cm-lp-callout-note", { hasText: "Inner" });
    await expect(nested).toBeVisible();
    expect(await page.locator(".cm-lp-callout-icon").count()).toBe(1);
  } finally {
    removeTempVault(vault);
  }
});

test("inline markdown in the body keeps live rendering", async () => {
  const vault = createTempVault({ "callout.md": "body start\n\n> [!note]\n> **bold** body\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /callout/ }).click();
    const body = page.locator(".cm-line.cm-lp-callout-note", { hasText: "bold body" });
    await expect(body.locator(".cm-lp-strong", { hasText: "bold" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});
