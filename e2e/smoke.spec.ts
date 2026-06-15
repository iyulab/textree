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

test("볼트 열기 → 트리에 최상위 노드 렌더", async () => {
  await loadVault(page, sampleVaultPath());

  // sample-vault 최상위: 프로젝트(리프) · 일기(컨테이너) · 자료실(컨테이너).
  await expect(page.getByRole("treeitem", { name: /프로젝트/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /일기/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /자료실/ })).toBeVisible();
});

test("노트 선택 → 본문이 에디터에 로드", async () => {
  await loadVault(page, sampleVaultPath());

  await page.getByRole("treeitem", { name: /프로젝트/ }).click();

  // 제목 헤더에 노트명, 본문은 CodeMirror에 렌더.
  await expect(page.locator(".title")).toContainText("프로젝트");
  await expect(page.locator(".cm-content")).toBeVisible();
});
