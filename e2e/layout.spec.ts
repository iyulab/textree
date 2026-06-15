import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * D2 — 앱 셸. 사이드바 접기/펼치기 + 리사이즈가 동작하고 선택이 localStorage 에
 * 영속하는지 실제 webview 에서 단언한다.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/** 사이드바를 펼쳐진(기본) 상태로 정규화. */
async function ensureExpanded(page: Page): Promise<void> {
  if (await page.locator(".expand-btn").isVisible().catch(() => false)) {
    await page.locator(".expand-btn").click();
  }
  await expect(page.locator(".sidebar")).toBeVisible();
}

test("사이드바 접기/펼치기 + 영속", async () => {
  await ensureExpanded(page);

  // 접기
  await page.getByRole("button", { name: "사이드바 접기" }).click();
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await expect(page.locator(".expand-btn")).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("true");

  // 펼치기
  await page.locator(".expand-btn").click();
  await expect(page.locator(".sidebar")).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-collapsed")),
  ).toBe("false");
});

test("사이드바 리사이즈 + 폭 영속", async () => {
  // 멱등성: 시작 폭을 좁은 baseline(220)으로 리셋 후 측정한다. 그러지 않으면
  // 직전 실행이 영속한 폭이 누적되어 최대치(480)에 닿으면 더 못 넓혀 실패한다.
  await page.evaluate(() => localStorage.setItem("textree-sidebar-width", "220"));
  await page.reload();
  await page.waitForFunction(() => (window as { __textreeTest?: unknown }).__textreeTest);
  await ensureExpanded(page);

  const sidebar = page.locator(".sidebar");
  const startW = (await sidebar.boundingBox())!.width;

  // 드래그 핸들을 오른쪽으로 끌어 폭을 넓힌다(목표 폭은 max 480 미만으로).
  const handle = page.locator(".resize-handle");
  const hb = (await handle.boundingBox())!;
  const y = hb.y + hb.height / 2;
  await page.mouse.move(hb.x + hb.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(startW + 120, y, { steps: 8 });
  await page.mouse.up();

  const endW = (await sidebar.boundingBox())!.width;
  expect(endW).toBeGreaterThan(startW + 40);

  // 폭이 localStorage 에 영속(드래그 종료 시 1회). 저장값은 layout.width(=clientX)와
  // 정확히 일치하지만, boundingBox(endW)는 스크롤바 거터만큼 측정이 달라질 수 있어
  // 스크롤바 폭 정도의 허용오차로 비교(앱의 실제 복원에는 드리프트 없음).
  const stored = Number(
    await page.evaluate(() => localStorage.getItem("textree-sidebar-width")),
  );
  expect(stored).toBeGreaterThan(startW + 40);
  expect(Math.abs(stored - endW)).toBeLessThan(20);
});
