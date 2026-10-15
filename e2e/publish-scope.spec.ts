import { expect, test, type Browser, type Page } from "@playwright/test";
import { existsSync, mkdtempSync, readFileSync, truncateSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  choosePublish,
  connectToApp,
  createTempVault,
  expectOpenNote,
  loadVault,
  recordNotes,
  removeTempVault,
  writeVaultFile,
} from "./helpers";

let browser: Browser;
let page: Page;
test.beforeAll(async () => ({ browser, page } = await connectToApp()));
test.afterAll(async () => browser?.close());

async function publishTo(p: Page, out: string) {
  const publishing = p.evaluate(
    (o) => (window as unknown as { __textreeTest: { publishTo: (o: string) => Promise<void> } }).__textreeTest.publishTo(o),
    out,
  );
  await expect(p.getByTestId("publish-confirm-go")).toBeEnabled({ timeout: 15000 });
  await p.getByTestId("publish-confirm-go").click();
  await publishing;
}

/** Opens the dialog asked before publishing; settles once it is answered. */
function openPublishDialog(p: Page): Promise<void> {
  return p.evaluate(() =>
    (window as unknown as { __textreeTest: { publishTo: (o: string) => Promise<void> } }).__textreeTest.publishTo(
      "C:/unused-out-dir",
    ),
  );
}

/** The sidebar row holding the item named `name` — the published marker sits beside the item. */
function rowOf(p: Page, name: RegExp) {
  return p.locator(".row").filter({ has: p.getByRole("treeitem", { name }) });
}

async function command(p: Page, query: string): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-overlay")).toBeVisible();
  await p.getByTestId("palette-input").fill(`>${query}`);
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

test("only chosen notes go out, as their last added version", async () => {
  const vault = createTempVault({
    "blog/post.md": "# Post\n\n![chart](chart.png)\n",
    "blog/chart.png": "png",
    "blog/draft.md": "# Draft\n",
    "diary.md": "# Diary\n",
  });
  const out = mkdtempSync(join(tmpdir(), "textree-pub-")).replace(/\\/g, "/");
  try {
    await loadVault(page, vault);
    // The picture is recorded together with the post that shows it.
    await recordNotes(page, vault, ["blog/post.md", "blog/draft.md", "diary.md"]);
    await choosePublish(page, vault, "blog", true);
    await choosePublish(page, vault, "blog/draft.md", false);
    writeVaultFile(vault, "blog/post.md", "# Post\n\nedited, not added\n![chart](chart.png)\n");
    await publishTo(page, out);
    await expect.poll(() => existsSync(join(out, "blog", "post.html")), { timeout: 15000 }).toBe(true);
    expect(existsSync(join(out, "diary.html"))).toBe(false);
    expect(existsSync(join(out, "blog", "draft.html"))).toBe(false);
    expect(readFileSync(join(out, "blog", "post.html"), "utf8")).not.toContain("edited, not added");
    expect(existsSync(join(out, "blog", "chart.png"))).toBe(true);
  } finally {
    removeTempVault(vault);
    removeTempVault(out);
  }
});

test("nothing chosen: the dialog offers the whole notes folder", async () => {
  const vault = createTempVault({ "a.md": "# A\n" });
  try {
    await loadVault(page, vault);
    await recordNotes(page, vault, ["a.md"]);
    const opening = openPublishDialog(page);
    await expect(page.getByTestId("publish-confirm-nothing-chosen")).toBeVisible();
    await expect(page.getByTestId("publish-confirm-go")).toBeDisabled();
    await page.getByTestId("publish-confirm-choose-all").click();
    await expect(page.getByTestId("publish-confirm-summary")).toContainText("1 note");
    await page.getByTestId("publish-confirm-cancel").click();
    await opening;
    // The sidebar learned of the choice made in the dialog.
    await expect(rowOf(page, /^a$/).getByTestId("tree-published")).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("notes with no version yet can be added from the dialog, and then go out", async () => {
  const vault = createTempVault({ "x.md": "# X\n", "y.md": "# Y\n![big](big.bin)\n", "big.bin": "" });
  // Over the size limit: never recorded, and named on a line of its own.
  truncateSync(join(vault, "big.bin"), 50 * 1024 * 1024 + 1);
  try {
    await loadVault(page, vault);
    const opening = openPublishDialog(page);
    await page.getByTestId("publish-confirm-choose-all").click();
    await expect(page.getByTestId("publish-confirm-never-recorded")).toContainText("x, y");
    await expect(page.getByTestId("publish-confirm-go")).toBeDisabled();
    await page.getByTestId("publish-confirm-add-versions").click();
    await expect(page.getByTestId("publish-confirm-summary")).toContainText("2 notes", { timeout: 15000 });
    await expect(page.getByTestId("publish-confirm-never-recorded")).toHaveCount(0);
    await expect(page.getByTestId("publish-confirm-too-large")).toContainText("big.bin");
    await expect(page.getByTestId("publish-confirm-missing")).toHaveCount(0);
    await page.getByTestId("publish-confirm-cancel").click();
    await opening;
  } finally {
    removeTempVault(vault);
  }
});

test("a note renamed inside an included folder keeps being left out in the sidebar", async () => {
  const vault = createTempVault({ "blog/post.md": "# Post\n", "blog/draft.md": "# Draft\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /blog/ }).click();
    await command(page, "Include in published site");
    await page.getByRole("treeitem", { name: /draft/ }).click();
    await command(page, "Leave out of published site");
    await expect(rowOf(page, /draft/).getByTestId("tree-published")).toHaveCount(0);
    await page.getByRole("treeitem", { name: /draft/ }).click();
    await page.getByRole("button", { name: "Rename", exact: true }).click();
    await page.locator(".tree-rename-input").fill("draft2");
    await page.locator(".tree-rename-input").press("Enter");
    await expect(page.getByRole("treeitem", { name: /draft2/ })).toBeVisible();
    await expect(rowOf(page, /post/).getByTestId("tree-published")).toBeVisible();
    await expect(rowOf(page, /draft2/).getByTestId("tree-published")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});

test("the sidebar marks what is included, and adding a version says so", async () => {
  const vault = createTempVault({ "blog/post.md": "# Post\n", "other.md": "# Other\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /blog/ }).click();
    await command(page, "Include in published site");
    // A note inside an included folder shows the marker too.
    await expect(rowOf(page, /blog/).getByTestId("tree-published")).toBeVisible();
    await expect(rowOf(page, /post/).getByTestId("tree-published")).toBeVisible();
    await expect(rowOf(page, /other/).getByTestId("tree-published")).toHaveCount(0);
    await expect(rowOf(page, /blog/).getByRole("img", { name: "Included in the published site" })).toBeVisible();
    await page.getByRole("treeitem", { name: /post/ }).click();
    await expectOpenNote(page, "post");
    await page.keyboard.press("Control+Shift+S");
    await expect(page.getByTestId("add-version-published")).toBeVisible();
    // Leave the dialog closed: an open one would block every spec after this.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("add-version")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});
