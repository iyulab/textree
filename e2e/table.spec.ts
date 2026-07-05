import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Editor GFM table live rendering. A pipe table renders as a <table> widget on inactive lines;
 * clicking a cell reveals the raw source with the cursor at that cell. Cells keep the inline set
 * the line pass renders (bold, code, wikilinks, inline math) — the no-regression contract.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// Line 1 = plain (default cursor) so the table below is inactive and renders.
const NOTE =
  "body start\n\n| Name | Value |\n| --- | ---: |\n| **bold** | 42 |\n| plain | $x^2$ |\n\nafter\n";

test("a pipe table renders as a table widget on inactive lines", async () => {
  const vault = createTempVault({ "table.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    const widget = page.locator(".cm-lp-table table");
    await expect(widget).toBeVisible();
    await expect(widget.locator("thead th")).toHaveCount(2);
    await expect(widget.locator("tbody tr")).toHaveCount(2);
    // The raw delimiter row is consumed by the widget (not visible as text).
    expect(await page.locator(".cm-content .cm-line", { hasText: "---" }).count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});

test("cells render bold and inline math (no-regression fidelity)", async () => {
  const vault = createTempVault({ "table.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    const widget = page.locator(".cm-lp-table");
    await expect(widget.locator(".cm-lp-strong", { hasText: "bold" })).toBeVisible();
    // Markers are hidden in the rendered cell.
    await expect(widget.locator("tbody")).not.toContainText("**");
    await expect(widget.locator(".cm-lp-math-inline .katex")).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("column alignment from the delimiter row applies", async () => {
  const vault = createTempVault({ "table.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    // Second column is `---:` -> right-aligned.
    const cell = page.locator(".cm-lp-table tbody td", { hasText: "42" });
    await expect(cell).toHaveCSS("text-align", "right");
  } finally {
    removeTempVault(vault);
  }
});

test("clicking a cell reveals the raw table with the cursor at that cell", async () => {
  const vault = createTempVault({ "table.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    const cell = page.locator(".cm-lp-table tbody td", { hasText: "42" });
    await expect(cell).toBeVisible();
    // Real mouse click at the cell's visual center (widget overlays the .cm-line, locator click
    // is refused as "intercepted" — same technique as the math widget tests).
    const box = await cell.boundingBox();
    if (!box) throw new Error("table cell has no bounding box");
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    // Raw source returns...
    await expect(page.locator(".cm-content .cm-line", { hasText: "**bold**" })).toBeVisible();
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
    // ...and the cursor landed at the clicked cell's source: typing goes into that cell.
    await page.keyboard.type("9");
    await expect(page.locator(".cm-content .cm-line", { hasText: "**bold**" })).toContainText(
      "942",
    );
  } finally {
    removeTempVault(vault);
  }
});

test("moving the cursor out re-renders the table", async () => {
  const vault = createTempVault({ "table.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    const cell = page.locator(".cm-lp-table tbody td", { hasText: "42" });
    const box = await cell.boundingBox();
    if (!box) throw new Error("table cell has no bounding box");
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
    // Click the plain line below the table -> cursor leaves -> the table renders again.
    await page.locator(".cm-content .cm-line", { hasText: "after" }).click();
    await expect(page.locator(".cm-lp-table table")).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("a resolved wikilink inside a cell navigates on click", async () => {
  const vault = createTempVault({
    "table.md": "body start\n\n| Link |\n| --- |\n| [[target]] |\n",
    "target.md": "target body\n",
  });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /table/ }).click();
    const link = page.locator(".cm-lp-table .cm-lp-wikilink");
    await expect(link).toBeVisible();
    const box = await link.boundingBox();
    if (!box) throw new Error("wikilink has no bounding box");
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    // Navigation (not reveal): the editor now shows the target note.
    await expect(page.locator(".cm-content")).toContainText("target body");
  } finally {
    removeTempVault(vault);
  }
});

test("a table inside a code fence stays raw", async () => {
  const FENCED = "before\n```\n| a | b |\n| --- | --- |\n| 1 | 2 |\n```\nafter\n";
  const vault = createTempVault({ "fence.md": FENCED });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /fence/ }).click();
    await expect(page.locator(".cm-content")).toContainText("after");
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
    await expect(page.locator(".cm-content .cm-line", { hasText: "| a | b |" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("pipe rows inside a $$ block stay math (no overlapping table widget)", async () => {
  const MATH_TABLE = "body start\n\n$$\n| a |\n| - |\n$$\n";
  const vault = createTempVault({ "mt.md": MATH_TABLE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /mt/ }).click();
    // The math field owns the block; the table field must not emit an overlapping widget.
    await expect(page.locator(".cm-lp-math-block")).toBeVisible();
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
  } finally {
    removeTempVault(vault);
  }
});

test("a table inside a blockquote stays raw (v1 scope)", async () => {
  const QUOTED = "body start\n\n> | a | b |\n> | --- | --- |\n> | 1 | 2 |\n";
  const vault = createTempVault({ "quoted.md": QUOTED });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /quoted/ }).click();
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
    await expect(page.locator(".cm-content")).toContainText("| a | b |");
  } finally {
    removeTempVault(vault);
  }
});

test("reading mode renders the table even on the cursor's line", async () => {
  const CURSOR_TABLE = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
  const vault = createTempVault({ "rt.md": CURSOR_TABLE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /rt/ }).click();
    // Edit mode: cursor starts on line 1 (inside the table) -> raw.
    expect(await page.locator(".cm-lp-table").count()).toBe(0);
    await page.getByRole("button", { name: "Switch to reading view" }).click();
    await expect(page.locator(".cm-lp-table table")).toBeVisible();
    await page.getByRole("button", { name: "Switch to editing" }).click();
    await expect(page.locator(".cm-content")).toHaveAttribute("contenteditable", "true");
  } finally {
    removeTempVault(vault);
  }
});
