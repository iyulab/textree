import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  createTempVault,
  git,
  loadVault,
  readSidecar,
  readVaultFile,
  removeTempVault,
} from "./helpers";
import { join } from "node:path";

/**
 * A notes folder inside a repository that holds other things (code beside the notes): the app keeps
 * its history in that repository, and everything the screen deals in is named relative to the
 * folder — not to the repository. Mixing the two would put history and settings in the wrong place.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

async function runCommand(p: Page, query: string): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-input")).toBeVisible();
  await p.getByTestId("palette-input").fill(query);
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

test("a folder inside a repository: notes, versions and settings are named relative to the folder", async () => {
  const repo = createTempVault({ "README.md": "code\n", "notes/top.md": "# top\n", "notes/sub/memo.md": "# memo\n\nfirst\n" });
  try {
    git(repo, "init", "-q");
    git(repo, "add", "README.md");
    git(repo, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "code");
    const head = git(repo, "rev-parse", "HEAD");
    const vault = join(repo, "notes");

    await loadVault(page, vault);
    await expect(page.getByRole("treeitem", { name: /top/ })).toBeVisible();
    await expect(page.getByRole("treeitem", { name: /README/ })).toHaveCount(0);

    await page.getByRole("treeitem", { name: /memo/ }).click();
    await expect(page.locator(".note-name")).toHaveText("memo");
    await expect(page.locator(".crumb")).toHaveText("sub");

    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("\ndraft one");
    await expect.poll(() => readVaultFile(vault, "sub/memo.md"), { timeout: 5000 }).toContain("draft one");

    await page.keyboard.press("Control+Shift+S");
    await expect(page.getByTestId("add-version")).toBeVisible();
    await page.getByTestId("add-version-confirm").click();
    await expect(page.getByTestId("add-version")).toHaveCount(0);
    await expect(page.getByTestId("add-version-error")).toHaveCount(0);

    // The history is the repository's, under the folder's place in it; the code's branch is untouched.
    expect(git(repo, "ls-tree", "-r", "--name-only", "refs/textree/notes").split("\n")).toContain("notes/sub/memo.md");
    expect(git(repo, "rev-parse", "HEAD")).toBe(head);

    await runCommand(page, ">Version history");
    await expect(page.getByTestId("version-entry")).toHaveCount(1);
    await page.keyboard.press("Escape");

    await page.getByRole("treeitem", { name: /memo/ }).click();
    await runCommand(page, ">favorite");
    await expect.poll(() => readSidecar(vault, "favorites.json")).toEqual(["sub/memo.md"]);
  } finally {
    removeTempVault(repo);
  }
});
