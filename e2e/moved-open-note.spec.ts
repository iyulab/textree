import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault, readVaultFile } from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

/**
 * Moving the open note keeps it open at its new place. What is on screen afterwards must be what
 * was typed — not the note as it was when it was opened — and typing on must not put the old
 * text back on disk.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

async function typeAndWaitSaved(vault: string, rel: string, text: string) {
  await page.locator(".cm-content").click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type(text);
  await expect.poll(() => readVaultFile(vault, rel)).toContain(text);
  await expect(page.getByText("Saved", { exact: true })).toBeVisible();
}

async function cutAndPaste(src: RegExp, dst: RegExp) {
  await page.getByRole("treeitem", { name: src }).focus();
  await page.keyboard.press("Control+x");
  await page.getByRole("treeitem", { name: dst }).focus();
  await page.keyboard.press("Control+v");
}

test("moving the open note into a folder keeps what was typed on screen and on disk", async () => {
  const vault = createTempVault({ "moving.md": "first line\n", "dest/other.md": "x\n" });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /moving/ }).click();
    await expect(page.locator(".cm-content")).toContainText("first line");
    await typeAndWaitSaved(vault, "moving.md", "typed before the move");

    await cutAndPaste(/moving/, /dest/);
    await expect.poll(() => existsSync(join(vault, "dest", "moving.md"))).toBe(true);

    await expect(page.locator(".cm-content")).toContainText("typed before the move");
    await typeAndWaitSaved(vault, "dest/moving.md", " and after");
    expect(readVaultFile(vault, "dest/moving.md")).toContain("typed before the move and after");
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("dropping a note onto the open note keeps what was typed in the open note", async () => {
  const vault = createTempVault({ "host.md": "host line\n", "guest.md": "g\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /host/ }).click();
    await expect(page.locator(".cm-content")).toContainText("host line");
    await typeAndWaitSaved(vault, "host.md", "typed in the host");

    await cutAndPaste(/guest/, /host/);
    await expect.poll(() => existsSync(join(vault, "host", "host.md"))).toBe(true);

    await expect(page.locator(".cm-content")).toContainText("typed in the host");
    await typeAndWaitSaved(vault, "host/host.md", " and after");
    expect(readVaultFile(vault, "host/host.md")).toContain("typed in the host and after");
  } finally {
    removeTempVault(vault);
  }
});

test("renaming the open note from its title keeps what was typed on screen and on disk", async () => {
  const vault = createTempVault({ "Before.md": "first line\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Before/ }).click();
    await expect(page.locator(".cm-content")).toContainText("first line");
    await typeAndWaitSaved(vault, "Before.md", "typed before the rename");

    await page.locator(".note-name").click();
    await page.locator(".title-input").fill("After");
    await page.locator(".title-input").press("Enter");
    await expect.poll(() => existsSync(join(vault, "After.md"))).toBe(true);
    await expect(page.locator(".note-name")).toHaveText("After");

    await typeAndWaitSaved(vault, "After.md", " and after");
    expect(readVaultFile(vault, "After.md")).toContain("typed before the rename and after");
  } finally {
    removeTempVault(vault);
  }
});

async function renameInTree(item: RegExp, name: string) {
  await page.getByRole("treeitem", { name: item }).click();
  await page.getByRole("button", { name: "Rename", exact: true }).click();
  await page.locator(".tree-rename-input").fill(name);
  await page.locator(".tree-rename-input").press("Enter");
}

test("renaming the open note in the tree keeps it open with what was typed", async () => {
  const vault = createTempVault({ "tree-old.md": "first line\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /tree-old/ }).click();
    await typeAndWaitSaved(vault, "tree-old.md", "typed before the rename");

    await renameInTree(/tree-old/, "tree-new");
    await expect.poll(() => existsSync(join(vault, "tree-new.md"))).toBe(true);
    await expect(page.locator(".note-name")).toHaveText("tree-new");

    await typeAndWaitSaved(vault, "tree-new.md", " and after");
    expect(readVaultFile(vault, "tree-new.md")).toContain("typed before the rename and after");
  } finally {
    removeTempVault(vault);
  }
});

test("renaming the folder that holds the open note keeps it open at its new place", async () => {
  const vault = createTempVault({ "box/inner.md": "first line\n" });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /inner/ }).click();
    await typeAndWaitSaved(vault, "box/inner.md", "typed before the rename");

    // By keyboard: clicking the folder would open it and leave the note.
    await page.getByRole("treeitem", { name: /box/ }).focus();
    await page.keyboard.press("F2");
    await page.locator(".tree-rename-input").fill("crate");
    await page.locator(".tree-rename-input").press("Enter");
    await expect.poll(() => existsSync(join(vault, "crate", "inner.md"))).toBe(true);
    await expect(page.locator(".note-name")).toHaveText("inner");

    await typeAndWaitSaved(vault, "crate/inner.md", " and after");
    expect(readVaultFile(vault, "crate/inner.md")).toContain("typed before the rename and after");
    expect(existsSync(join(vault, "box"))).toBe(false);
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});
