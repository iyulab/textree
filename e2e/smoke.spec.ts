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

  // sample-vault top level: 프로젝트 (leaf) · 일기 (container) · 자료실 (container).
  await expect(page.getByRole("treeitem", { name: /프로젝트/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /일기/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /자료실/ })).toBeVisible();
});

test("select note -> body loads in editor", async () => {
  await loadVault(page, sampleVaultPath());

  await page.getByRole("treeitem", { name: /프로젝트/ }).click();

  // Note name in the title header, body renders in CodeMirror.
  await expect(page.locator(".title")).toContainText("프로젝트");
  await expect(page.locator(".cm-content")).toBeVisible();
});
