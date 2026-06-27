import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

/**
 * D5 — Page detail. Asserts that clicking the title in the body header to edit
 * it inline actually renames the note (file) and updates the title.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("inline title edit -> file rename + title update", async () => {
  const vault = createTempVault({ "원제목.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /원제목/ }).click();
    await expect(page.locator(".note-name")).toHaveText("원제목");

    // Click title -> input -> enter new name -> Enter.
    await page.locator(".note-name").click();
    await expect(page.locator(".title-input")).toBeVisible();
    await page.locator(".title-input").fill("새제목");
    await page.locator(".title-input").press("Enter");

    // Title updated + disk file renamed.
    await expect(page.locator(".note-name")).toHaveText("새제목");
    await expect(page.getByRole("treeitem", { name: /새제목/ })).toBeVisible();
    expect(existsSync(join(vault, "새제목.md"))).toBe(true);
    expect(existsSync(join(vault, "원제목.md"))).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("title edit Escape -> no change", async () => {
  const vault = createTempVault({ "유지.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /유지/ }).click();

    await page.locator(".note-name").click();
    await page.locator(".title-input").fill("바뀐이름");
    await page.locator(".title-input").press("Escape");

    await expect(page.locator(".note-name")).toHaveText("유지");
    expect(existsSync(join(vault, "유지.md"))).toBe(true);
    expect(existsSync(join(vault, "바뀐이름.md"))).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

// Chrome-on-demand (design principle): the header's tool cluster (save status, reading
// toggle, Chat entry) is decoration — hidden at rest so a captured note shows just its
// title, revealed on hover/focus. Guards the "clean capture" success criterion against
// regressions in the .title-tools styling.
test("chrome-on-demand: header tools hide until hover/focus; inline Chat entry present", async () => {
  const vault = createTempVault({ "Visible.md": "# Visible\n\nbody\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Visible/ }).click();
    await expect(page.locator("header.title")).toBeVisible();

    const tools = page.locator(".title-tools");

    // Idle (mouse away + nothing focused in the header): tools hidden → clean capture.
    await page.mouse.move(5, 5);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await expect(tools).toHaveCSS("opacity", "0");

    // Hovering the header reveals the tools.
    await page.locator("header.title").hover();
    await expect(tools).toHaveCSS("opacity", "1");

    // Inline Chat-entry affordance exists with an accessible label.
    const chatBtn = page.getByRole("button", { name: "Switch to chat" });
    await expect(chatBtn).toBeVisible();

    // Keyboard a11y: focusing a tool reveals the group via :focus-within (not hover-only),
    // so the hidden controls remain reachable without a mouse.
    await page.mouse.move(5, 5);
    await chatBtn.focus();
    await expect(tools).toHaveCSS("opacity", "1");
  } finally {
    removeTempVault(vault);
  }
});
