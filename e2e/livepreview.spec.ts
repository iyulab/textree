import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * D4 — 라이브 프리뷰. 헤딩/강조/코드가 인라인 렌더되고, 마커(#, **, `)는 커서가
 * 놓이지 않은 줄에서 숨겨지며 커서가 그 줄에 오면 다시 보이는지 단언한다.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// 1행=평문(기본 커서 위치) → 2행 헤딩·3행 강조/코드는 비활성 줄이라 마커가 숨겨진다.
const NOTE = "본문 시작\n# 큰제목\n**굵게** 그리고 `코드` 그리고 ~~삭제~~\n";

test("헤딩/강조/코드 인라인 렌더 + 마커 숨김", async () => {
  const vault = createTempVault({ "lp.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /lp/ }).click();

    // 데코 DOM 존재.
    await expect(page.locator(".cm-lp-h1")).toBeVisible();
    await expect(page.locator(".cm-lp-strong")).toHaveText("굵게");
    await expect(page.locator(".cm-lp-code")).toHaveText("코드");
    await expect(page.locator(".cm-lp-strike")).toHaveText("삭제");

    // 비활성 줄: 헤딩 마커(#) 숨김 → 줄 텍스트는 "큰제목"(# 없음).
    await expect(page.locator(".cm-lp-h1")).toHaveText("큰제목");
  } finally {
    removeTempVault(vault);
  }
});

// 1행=평문(활성) → 2행 이하 LP 렌더.
const RICH =
  "본문\n[링크텍스트](https://example.com)\n- [ ] 할일\n> 인용문\n---\n끝\n";

test("링크·체크박스·인용·구분선 렌더", async () => {
  const vault = createTempVault({ "rich.md": RICH });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /rich/ }).click();

    // 링크: 텍스트만 표시(URL 숨김).
    await expect(page.locator(".cm-lp-link")).toHaveText("링크텍스트");

    // 인용/구분선 데코.
    await expect(page.locator(".cm-lp-quote")).toBeVisible();
    await expect(page.locator(".cm-lp-hr")).toBeVisible();

    // 체크박스: 미체크 → 클릭 → 체크(원문 [ ]→[x] 토글).
    const box = page.locator(".cm-lp-checkbox");
    await expect(box).not.toBeChecked();
    await box.click();
    await expect(page.locator(".cm-lp-checkbox")).toBeChecked();
  } finally {
    removeTempVault(vault);
  }
});

test("커서가 놓인 줄은 마커(소스) 노출", async () => {
  const vault = createTempVault({ "lp.md": NOTE });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /lp/ }).click();

    // 헤딩 줄 클릭 → 그 줄이 활성 → 마커(#) 다시 보임.
    await page.locator(".cm-lp-h1").click();
    await expect(page.locator(".cm-lp-h1")).toContainText("#");
  } finally {
    removeTempVault(vault);
  }
});
