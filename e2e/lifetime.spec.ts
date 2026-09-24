import { test, expect, type Browser, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  writeVaultFile,
  E2E_WATCHDOG_INTERVAL_MS,
} from "./helpers";

/**
 * A folder left open in the running app, for long enough that everything the app does in the
 * background has happened several times over.
 *
 * Opening and reading are covered elsewhere; this is about what runs on its own afterwards — the
 * watcher's liveness probe above all, which used to write into the folder every few minutes. The
 * launcher shortens its interval (E2E_WATCHDOG_INTERVAL_MS) so a few seconds here are several
 * whole probe cycles.
 *
 * Three folders, because where the app may write differs for each: a repository's root (the
 * probe goes into repository storage), a folder inside someone else's repository (nowhere is
 * ours), and a folder with no repository yet (nowhere is ours, and writing a note still has to
 * be atomic).
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/** Several probe cycles, plus the debounce, with room to spare. */
const SEVERAL_PROBES = E2E_WATCHDOG_INTERVAL_MS * 3 + 1500;

function git(repo: string, ...args: string[]): string {
  return execFileSync("git", ["-c", "core.autocrlf=false", "-C", repo, ...args], { encoding: "utf8" }).trim();
}

function someonesRepository(files: Record<string, string>): string {
  const repo = createTempVault(files);
  git(repo, "init", "-q");
  git(repo, "add", ".");
  git(repo, "-c", "user.name=someone", "-c", "user.email=someone@example.com", "commit", "-qm", "their work");
  return repo;
}

test("lifetime: a repository left open stays exactly as its owner left it, and changes still show up", async () => {
  const repo = someonesRepository({ "README.md": "# code\n", "main.rs": "fn main() {}\n", "notes/plan.md": "# plan\n" });
  try {
    await loadVault(page, repo);
    await page.waitForTimeout(SEVERAL_PROBES);

    expect(git(repo, "status", "--porcelain", "--ignored"), "nothing new in their working tree").toBe("");
    expect(readdirSync(repo).sort()).toEqual([".git", "README.md", "main.rs", "notes"]);

    // The watcher is still alive after the probes: a change made outside the app arrives.
    writeVaultFile(repo, "notes/added-outside.md", "# added outside\n");
    await expect(page.getByRole("treeitem", { name: /added-outside/ })).toBeVisible({ timeout: 5000 });
  } finally {
    removeTempVault(repo);
  }
});

test("lifetime: a folder inside someone's repository is never written into", async () => {
  const repo = someonesRepository({ "src/lib.rs": "\n", "docs/guide.md": "# guide\n" });
  try {
    await loadVault(page, join(repo, "docs").replace(/\\/g, "/"));
    await page.waitForTimeout(SEVERAL_PROBES);

    expect(git(repo, "status", "--porcelain", "--ignored")).toBe("");
    expect(readdirSync(join(repo, "docs"))).toEqual(["guide.md"]);
  } finally {
    removeTempVault(repo);
  }
});

test("lifetime: a folder nothing was recorded in holds only its notes after editing", async () => {
  const vault = createTempVault({ "memo.md": "first line\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /memo/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("second line");
    await expect.poll(() => readVaultFile(vault, "memo.md"), { timeout: 5000 }).toContain("second line");

    await page.waitForTimeout(SEVERAL_PROBES);
    expect(readdirSync(vault), "no repository, no directory of ours, no leftover temp").toEqual(["memo.md"]);
  } finally {
    removeTempVault(vault);
  }
});
