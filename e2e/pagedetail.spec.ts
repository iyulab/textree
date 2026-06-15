import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

/**
 * D5 — 페이지 디테일. 본문 헤더의 제목을 클릭해 인라인 편집하면 노트(파일)가
 * 실제로 rename 되고 제목이 갱신되는지 단언한다.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("인라인 제목 편집 → 파일 rename + 제목 갱신", async () => {
  const vault = createTempVault({ "원제목.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /원제목/ }).click();
    await expect(page.locator(".note-name")).toHaveText("원제목");

    // 제목 클릭 → 입력창 → 새 이름 입력 → Enter.
    await page.locator(".note-name").click();
    await expect(page.locator(".title-input")).toBeVisible();
    await page.locator(".title-input").fill("새제목");
    await page.locator(".title-input").press("Enter");

    // 제목 갱신 + 디스크 파일 rename.
    await expect(page.locator(".note-name")).toHaveText("새제목");
    await expect(page.getByRole("treeitem", { name: /새제목/ })).toBeVisible();
    expect(existsSync(join(vault, "새제목.md"))).toBe(true);
    expect(existsSync(join(vault, "원제목.md"))).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("제목 편집 Escape → 변경 없음", async () => {
  const vault = createTempVault({ "유지.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /유지/ }).click();

    await page.locator(".note-name").click();
    await page.locator(".title-input").fill("바뀐이름");
    await page.locator(".title-input").press("Escape");

    await expect(page.locator(".note-name")).toHaveText("유지");
    expect(existsSync(join(vault, "유지.md"))).toBe(true);
    expect(existsSync(join(vault, "바뀐이름.md"))).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});
