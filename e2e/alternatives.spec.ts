import { test, expect, type Browser, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  expectOpenNote,
} from "./helpers";

/**
 * An alternative of a note: start one, work on it beside the note, compare word by word, and
 * either use it or set it aside. The note's file changes only when the alternative is used.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

async function command(p: Page, query: string): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-overlay")).toBeVisible();
  await p.getByTestId("palette-input").fill(query);
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

async function addVersion(p: Page): Promise<void> {
  await p.keyboard.press("Control+Shift+S");
  await expect(p.getByTestId("add-version")).toBeVisible();
  await p.getByTestId("add-version-confirm").click();
  await expect(p.getByTestId("add-version")).toHaveCount(0);
}

/** Opens `name` and starts an alternative of it, with `text` added to the alternative. */
async function startWith(p: Page, vault: string, name: string, text: string): Promise<void> {
  await loadVault(p, vault);
  await p.getByRole("treeitem", { name: new RegExp(name) }).click();
  await expectOpenNote(p, name);
  await addVersion(p);
  await command(p, ">start an alternative");
  const panel = p.getByTestId("alternatives");
  await expect(panel).toBeVisible();
  await p.getByTestId("alternative-edit").click();
  await p.getByTestId("alternative-editor").fill(text);
  await p.getByTestId("alternative-add-version").click();
  await expect(p.getByTestId("alternative-editor")).toHaveCount(0);
}

function refsUnder(vault: string, prefix: string): string[] {
  return execFileSync("git", ["-C", vault, "for-each-ref", "--format=%(refname)", prefix], { encoding: "utf8" })
    .split("\n")
    .filter(Boolean);
}

test("alternatives: start one, compare word by word, use it", async () => {
  const vault = createTempVault({ "plan.md": "# Plan\n\nWe meet on Monday at noon.\n" });
  try {
    await startWith(page, vault, "plan", "# Plan\n\nWe meet on Tuesday at noon.\n");

    // Only the changed word is marked, on each side.
    await expect(page.getByTestId("alternative-current").locator("del")).toHaveText(["Monday"]);
    await expect(page.getByTestId("alternative-text").locator("ins")).toHaveText(["Tuesday"]);
    // The note itself has not changed.
    expect(readVaultFile(vault, "plan.md")).toContain("Monday");

    await page.getByTestId("alternative-use-alternative").click();
    await expect(page.getByTestId("alternatives")).toHaveCount(0);
    await expect.poll(() => readVaultFile(vault, "plan.md")).toContain("Tuesday");
    await expect(page.locator(".cm-content")).toContainText("Tuesday");
    await expect(page.getByTestId("alternative-notice")).toHaveCount(0);
    expect(refsUnder(vault, "refs/textree/alternatives/")).toEqual([]);
    expect(refsUnder(vault, "refs/textree/ended/")).toHaveLength(1);
  } finally {
    removeTempVault(vault);
  }
});

test("alternatives: set one aside and the note stays; an open one is announced on the note", async () => {
  const vault = createTempVault({ "idea.md": "# Idea\n\nShort version.\n" });
  try {
    await startWith(page, vault, "idea", "# Idea\n\nA much longer version.\n");

    // Closed, the note says it has an alternative, and Compare opens it again.
    await page.getByRole("button", { name: "Close comparison" }).click();
    await expect(page.getByTestId("alternative-notice")).toContainText("You have an alternative of this note.");
    await page.getByTestId("alternative-compare").click();
    await expect(page.getByTestId("alternative-text")).toContainText("much longer");

    await page.getByTestId("alternative-set-aside").click();
    await expect(page.getByTestId("alternatives")).toHaveCount(0);
    await expect(page.getByTestId("alternative-notice")).toHaveCount(0);
    expect(readVaultFile(vault, "idea.md")).toContain("Short version.");
    // Set aside, not gone: it is kept under the ended ones.
    expect(refsUnder(vault, "refs/textree/ended/")).toHaveLength(1);
  } finally {
    removeTempVault(vault);
  }
});

test("alternatives: a note never added as a version says so instead of starting one", async () => {
  const vault = createTempVault({ "fresh.md": "# Fresh\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /fresh/ }).click();
    await expectOpenNote(page, "fresh");
    await command(page, ">start an alternative");
    await expect(page.getByTestId("version-notice")).toContainText("add a version of this note first");
    await expect(page.getByTestId("alternatives")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});
