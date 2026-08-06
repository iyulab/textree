import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("open vault -> top-level nodes render in tree", async () => {
  await loadVault(page, sampleVaultPath());

  // sample-vault top level: project (leaf) · journal (container) · library (container).
  await expect(page.getByRole("treeitem", { name: /project/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /journal/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /library/ })).toBeVisible();
});

test("select note -> body loads in editor", async () => {
  await loadVault(page, sampleVaultPath());

  await page.getByRole("treeitem", { name: /project/ }).click();

  // Note name in the title header, body renders in CodeMirror.
  await expect(page.locator(".title")).toContainText("project");
  await expect(page.locator(".cm-content")).toBeVisible();
});
