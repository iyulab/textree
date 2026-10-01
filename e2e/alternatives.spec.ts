import { test, expect, type Browser, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  expectOpenNote,
  listVaultDir,
  sidecarDir,
  addVersion,
  refsUnder,
} from "./helpers";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

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

/** Replaces what the editor shows with `text`. */
async function typeInEditor(p: Page, text: string): Promise<void> {
  await p.locator(".cm-content").click();
  await p.keyboard.press("Control+a");
  await p.keyboard.insertText(text);
}

/**
 * Opens `name` and starts an alternative of it — which opens it in the editor in place of the
 * note — with `text` added to the alternative, then compares the two.
 */
async function startWith(p: Page, vault: string, name: string, text: string): Promise<void> {
  await loadVault(p, vault);
  await p.getByRole("treeitem", { name: new RegExp(name) }).click();
  await expectOpenNote(p, name);
  await addVersion(p);
  await command(p, ">start an alternative");
  await expect(p.getByTestId("alternative-viewing")).toContainText("writing the alternative");
  await typeInEditor(p, text);
  await p.getByTestId("alternative-add-version").click();
  await expect(p.getByTestId("version-notice")).toContainText("Version added to the alternative.");
  await p.getByTestId("alternative-compare").click();
  await expect(p.getByTestId("alternatives")).toBeVisible();
}

/** The notes whose alternative drafts are kept for `vault`, outside it. */
function draftsKept(vault: string): string[] {
  const dir = join(sidecarDir(vault), "drafts");
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((id) => readdirSync(join(dir, id)));
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

    // Back on the note, it says it has an alternative, and Compare opens it again.
    await page.getByRole("button", { name: "Close comparison" }).click();
    await page.getByTestId("alternative-back").click();
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

test("alternatives: go back and forth — what is typed stays with the alternative, the note is untouched", async () => {
  const vault = createTempVault({ "draft.md": "# Draft\n\nThe note as it is.\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /draft/ }).click();
    await expectOpenNote(page, "draft");
    await addVersion(page);
    await command(page, ">start an alternative");
    await expect(page.getByTestId("alternative-viewing")).toBeVisible();

    // Typed, never added as a version, and nothing asked on the way back.
    await typeInEditor(page, "# Draft\n\nA different take, half done.\n");
    await page.getByTestId("alternative-back").click();
    await expect(page.getByTestId("alternative-viewing")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText("The note as it is.");
    expect(readVaultFile(vault, "draft.md")).toContain("The note as it is.");
    // Kept outside the folder, under the alternative.
    await expect.poll(() => draftsKept(vault)).toEqual(["draft.md"]);
    expect(listVaultDir(vault, "").sort()).toEqual([".git", "draft.md"]);

    // Coming back picks it up where it was left.
    await expect(page.getByTestId("alternative-notice")).toContainText("You have an alternative of this note.");
    await page.getByTestId("alternative-open").click();
    await expect(page.locator(".cm-content")).toContainText("A different take, half done.");

    // Set aside from the comparison: the ended alternative holds what was typed, and the draft goes.
    await page.getByTestId("alternative-compare").click();
    await expect(page.getByTestId("alternative-text")).toContainText("half done");
    await page.getByTestId("alternative-set-aside").click();
    await expect(page.getByTestId("alternative-viewing")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText("The note as it is.");
    expect(readVaultFile(vault, "draft.md")).toContain("The note as it is.");
    const [ended] = refsUnder(vault, "refs/textree/ended/");
    expect(execFileSync("git", ["-C", vault, "show", `${ended}:draft.md`], { encoding: "utf8" })).toContain(
      "half done",
    );
    await expect.poll(() => draftsKept(vault)).toEqual([]);
  } finally {
    removeTempVault(vault);
  }
});

test("alternatives: the page header shows the title of what is open — the alternative's, then the note's", async () => {
  const vault = createTempVault({ "plan.md": "---\ntitle: Plan A\n---\n\nThe first plan.\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /plan/ }).click();
    await expectOpenNote(page, "plan");
    await expect(page.locator(".page-title")).toHaveText("Plan A");
    await addVersion(page);
    await command(page, ">start an alternative");
    await expect(page.getByTestId("alternative-viewing")).toBeVisible();

    await typeInEditor(page, "---\ntitle: Plan B\n---\n\nA second plan.\n");
    await expect(page.locator(".page-title")).toHaveText("Plan B");

    await page.getByTestId("alternative-back").click();
    await expect(page.getByTestId("alternative-viewing")).toHaveCount(0);
    await expect(page.locator(".page-title")).toHaveText("Plan A");
    expect(readVaultFile(vault, "plan.md")).toContain("title: Plan A");
  } finally {
    removeTempVault(vault);
  }
});

test("alternatives: when what was typed cannot be kept, another note does not open until it can be", async () => {
  const vault = createTempVault({ "plan.md": "# Plan\n\nThe first plan.\n", "other.md": "# Other\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /plan/ }).click();
    await expectOpenNote(page, "plan");
    await addVersion(page);
    await command(page, ">start an alternative");
    await expect(page.getByTestId("alternative-viewing")).toBeVisible();

    // A folder where the draft goes makes every write of it fail.
    const [ref] = refsUnder(vault, "refs/textree/alternatives/");
    const blocker = join(sidecarDir(vault), "drafts", ref.split("/").pop() ?? "", "plan.md");
    mkdirSync(blocker, { recursive: true });
    writeFileSync(join(blocker, "keep"), "");

    await typeInEditor(page, "# Plan\n\nA second plan, only on screen.\n");
    await expect(page.getByTestId("alternative-failure")).toBeVisible({ timeout: 10_000 });
    await page.getByRole("treeitem", { name: /other/ }).click();
    // Still here: leaving would have dropped the only copy of what was typed.
    await expect(page.getByTestId("alternative-viewing")).toBeVisible();
    await expect(page.locator(".cm-content")).toContainText("only on screen");

    // Once it can be written, leaving writes it first.
    rmSync(blocker, { recursive: true, force: true });
    await page.getByRole("treeitem", { name: /other/ }).click();
    await expectOpenNote(page, "other");
    await expect(page.getByTestId("alternative-viewing")).toHaveCount(0);
    expect(readFileSync(blocker, "utf8")).toContain("only on screen");
    expect(readVaultFile(vault, "plan.md")).toContain("The first plan.");
  } finally {
    removeTempVault(vault);
  }
});
