import { expect, test, type Browser, type Page } from "@playwright/test";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
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
    const opening = page.evaluate(() =>
      (window as unknown as { __textreeTest: { publishTo: (o: string) => Promise<void> } }).__textreeTest.publishTo(
        "C:/unused-out-dir",
      ),
    );
    await expect(page.getByTestId("publish-confirm-nothing-chosen")).toBeVisible();
    await expect(page.getByTestId("publish-confirm-go")).toBeDisabled();
    await page.getByTestId("publish-confirm-choose-all").click();
    await expect(page.getByTestId("publish-confirm-summary")).toContainText("1 note");
    await page.getByTestId("publish-confirm-cancel").click();
    await opening;
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
    // The marker sits in the row beside the item; a note inside an included folder shows it too.
    const rowOf = (name: RegExp) => page.locator(".row").filter({ has: page.getByRole("treeitem", { name }) });
    await expect(rowOf(/blog/).getByTestId("tree-published")).toBeVisible();
    await expect(rowOf(/post/).getByTestId("tree-published")).toBeVisible();
    await expect(rowOf(/other/).getByTestId("tree-published")).toHaveCount(0);
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
