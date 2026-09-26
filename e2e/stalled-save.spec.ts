import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  listVaultDir,
  stallWrites,
  releaseWrites,
} from "./helpers";

// A folder that stops answering — a stalled sync or network drive — holds a save without failing
// it. The development build holds every write while the test says so (see stallWrites).

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  releaseWrites();
  await browser?.close();
});

test("a save the folder does not take: the note says so, another note opens, and the edits land once it answers", async () => {
  const vault = createTempVault({
    "slow.md": "before\n",
    "other.md": "# other\n",
    "third.md": "# third\n",
  });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /slow/ }).click();
    await expect(page.locator(".cm-content")).toContainText("before");

    stallWrites();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type(" typed");

    // Said after a while, not at once: a slow save is not a stalled one.
    await expect(page.getByTestId("save-status")).toHaveText(/Saving/);
    await expect(page.getByTestId("save-status")).toHaveText(/Still saving — the folder isn't answering/, {
      timeout: 15_000,
    });
    expect(readVaultFile(vault, "slow.md")).toBe("before\n");

    // Leaving is not held by it: the save goes on without the note on screen.
    await page.getByRole("treeitem", { name: /other/ }).click();
    await expect(page.locator(".cm-content")).toContainText("other");
    await expect(page.getByTestId("save-status")).toHaveText("Saved");
    // Nor is leaving the note after it, left untouched.
    await page.getByRole("treeitem", { name: /third/ }).click();
    await expect(page.locator(".cm-content")).toContainText("third");

    releaseWrites();
    await expect.poll(() => readVaultFile(vault, "slow.md")).toMatch(/^before\s*typed/);
    // Landed once, where it belongs — no copy beside it.
    expect(listVaultDir(vault, "").sort()).toEqual(["other.md", "slow.md", "third.md"]);
  } finally {
    releaseWrites();
    removeTempVault(vault);
  }
});
