import { test, expect, type Browser, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { connectToApp, loadVault, createTempVault, removeTempVault, readVaultFile, writeVaultFile } from "./helpers";

/**
 * Opening a folder that is already someone's git repository, and adding a version in it.
 *
 * A code repository that also holds notes is a supported vault. Adding a version there must leave
 * everything the owner of that repository relies on exactly as it was: the checked-out branch and
 * every branch tip, what they have staged, what they have not tracked, and the folder's contents.
 * The version lands under the app's own refs, recorded with the repository's author and the app as
 * committer.
 *
 * The unit gates prove this for the engine; this drives it through the screens.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

function git(repo: string, ...args: string[]): string {
  return execFileSync("git", ["-c", "core.autocrlf=false", "-C", repo, ...args], { encoding: "utf8" }).trim();
}

/** Everything about the repository its owner would notice changing. */
function ownerView(repo: string) {
  return {
    branch: git(repo, "branch", "--show-current"),
    head: git(repo, "rev-parse", "HEAD"),
    branches: git(repo, "for-each-ref", "--format=%(refname) %(objectname)", "refs/heads"),
    staged: git(repo, "diff", "--cached", "--name-status"),
    untracked: git(repo, "ls-files", "--others", "--exclude-standard"),
    entries: readdirSync(repo).sort(),
  };
}

test("existing repository: adding a version leaves branch, staging and folder untouched", async () => {
  const repo = createTempVault({
    "docs/design.md": "# design\n\nFirst draft.\n",
    "src/main.rs": "fn main() {}\n",
    "README.md": "readme\n",
  });

  try {
    git(repo, "init", "-q", "-b", "main");
    git(repo, "config", "user.name", "Repo Owner");
    git(repo, "config", "user.email", "owner@example.com");
    git(repo, "add", "-A");
    git(repo, "commit", "-q", "-m", "init");
    git(repo, "checkout", "-q", "-b", "feature");
    // Work in progress the owner has staged, and a file they have not tracked.
    writeVaultFile(repo, "src/main.rs", "fn main() { println!(\"wip\"); }\n");
    git(repo, "add", "src/main.rs");
    writeVaultFile(repo, "scratch.txt", "untracked\n");

    const before = ownerView(repo);

    await loadVault(page, repo);
    await page.getByRole("treeitem", { name: /design/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("\nSecond thought.");
    await expect.poll(() => readVaultFile(repo, "docs/design.md"), { timeout: 5000 }).toContain("Second thought.");

    await page.keyboard.press("Control+Shift+S");
    await expect(page.getByTestId("add-version")).toBeVisible();
    await page.getByTestId("add-version-name").fill("second thought");
    await page.getByTestId("add-version-confirm").click();
    await expect(page.getByTestId("add-version")).toHaveCount(0);
    await expect(page.getByTestId("add-version-error")).toHaveCount(0);

    // The version exists, under the app's own refs only.
    const appRefs = git(repo, "for-each-ref", "--format=%(refname)", "refs/textree");
    expect(appRefs).not.toBe("");
    const tip = appRefs.split("\n")[0];
    expect(git(repo, "log", "-1", "--format=%s", tip)).toBe("second thought");
    expect(git(repo, "log", "-1", "--format=%an <%ae>", tip)).toBe("Repo Owner <owner@example.com>");
    expect(git(repo, "log", "-1", "--format=%cn", tip)).not.toBe("Repo Owner");
    expect(git(repo, "show", `${tip}:docs/design.md`)).toContain("Second thought.");

    // Nothing the owner relies on moved.
    const after = ownerView(repo);
    expect(after.branch).toBe(before.branch);
    expect(after.head).toBe(before.head);
    expect(after.branches).toBe(before.branches);
    expect(after.staged).toBe(before.staged);
    expect(after.untracked).toBe(before.untracked);
    expect(after.entries).toEqual(before.entries);
  } finally {
    removeTempVault(repo);
  }
});
