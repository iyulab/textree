import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

/**
 * D3 — tree expand/collapse. Toggle child visibility via the container's chevron
 * and assert in a real webview that the state persists to localStorage.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("expand/collapse children via container chevron + persist", async () => {
  const vault = createTempVault({
    "폴더/자식1.md": "a\n",
    "폴더/자식2.md": "b\n",
  });
  try {
    // Reset so the tree collapsed state is not polluted by the previous test.
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    // Default: expanded → children visible.
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();
    await expect(page.getByRole("treeitem", { name: /자식2/ })).toBeVisible();

    // Collapse → children disappear. (exact: distinguish from the "Collapse sidebar" button)
    await page.getByRole("button", { name: "Collapse", exact: true }).click();
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /자식2/ })).toHaveCount(0);

    // Collapsed state persists (1 path stored).
    const stored = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("textree-tree-collapsed") ?? "[]"),
    );
    expect(stored.length).toBe(1);

    // Expand → children return.
    await page.getByRole("button", { name: "Expand", exact: true }).click();
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("keyboard: ↑↓ move · ←→ collapse/expand · Enter open", async () => {
  const vault = createTempVault({
    "폴더/자식1.md": "a\n",
    "폴더/자식2.md": "b\n",
  });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    const folder = page.getByRole("treeitem", { name: /폴더/ });
    await folder.focus();

    // ← collapse: hide children.
    await page.keyboard.press("ArrowLeft");
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toHaveCount(0);

    // → expand: children return.
    await page.keyboard.press("ArrowRight");
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();

    // ↓ move: focus moves to the next item (자식1).
    await page.keyboard.press("ArrowDown");
    const focusedText = await page.evaluate(
      () => document.activeElement?.textContent ?? "",
    );
    expect(focusedText).toContain("자식1");

    // Enter: open the focused note.
    await page.keyboard.press("Enter");
    await expect(page.locator(".note-name")).toHaveText("자식1");
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("keyboard: F2 enter rename mode · Delete delete", async () => {
  const vault = createTempVault({ "지울노트.md": "x\n", "남길노트.md": "y\n" });
  try {
    await loadVault(page, vault);

    // F2 → rename input appears (select + mode).
    await page.getByRole("treeitem", { name: /지울노트/ }).focus();
    await page.keyboard.press("F2");
    await expect(page.locator(".name-input")).toBeVisible();
    await page.keyboard.press("Escape");

    // Delete → to trash (disappears from tree).
    await page.getByRole("treeitem", { name: /지울노트/ }).focus();
    await page.keyboard.press("Delete");
    await expect(page.getByRole("treeitem", { name: /지울노트/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /남길노트/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("keyboard move: Ctrl+X cut → Ctrl+V move into folder", async () => {
  const vault = createTempVault({
    "이동노트.md": "a\n",
    "대상폴더/기존.md": "b\n",
  });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /이동노트/ }).focus();
    await page.keyboard.press("Control+x");
    await page.getByRole("treeitem", { name: /대상폴더/ }).focus();
    await page.keyboard.press("Control+v");

    // Disk: moved into 대상폴더.
    await expect
      .poll(() => existsSync(join(vault, "대상폴더", "이동노트.md")))
      .toBe(true);
    expect(existsSync(join(vault, "이동노트.md"))).toBe(false);
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("keyboard adopt: Ctrl+V on leaf → promote then child", async () => {
  const vault = createTempVault({ "소스.md": "a\n", "리프.md": "b\n" });
  try {
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /소스/ }).focus();
    await page.keyboard.press("Control+x");
    await page.getByRole("treeitem", { name: /리프/ }).focus();
    await page.keyboard.press("Control+v");

    // 리프 is promoted to a container and 소스 moves into it (adopt).
    await expect.poll(() => existsSync(join(vault, "리프", "소스.md"))).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("breadcrumb: show ancestor folder of a nested note", async () => {
  const vault = createTempVault({ "폴더/자식1.md": "a\n" });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /자식1/ }).click();
    // Body header shows the ancestor folder (폴더) + note name (자식1).
    await expect(page.locator(".crumb")).toHaveText("폴더");
    await expect(page.locator(".note-name")).toHaveText("자식1");
  } finally {
    removeTempVault(vault);
  }
});
