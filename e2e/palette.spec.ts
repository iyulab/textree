import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * P1a — 통합 팔레트. Ctrl+P로 열고 파일 검색(파일 모드)·명령 실행(명령 모드)을
 * 실제 WebView2에서 단언한다.
 *
 * 파일 모드: 기본, 쿼리가 '>'로 시작하지 않을 때.
 * 명령 모드: 쿼리가 '>'로 시작할 때. 실제 검색어는 '>' 이후.
 *
 * sample-vault 최상위 파일: 프로젝트(리프).
 * 명령 타이틀: "테마 전환(라이트↔다크)" → '>테마'로 fuzzy 매칭.
 */

let browser: Browser;
let page: Page;

const opposite = (t: string) => (t === "dark" ? "light" : "dark");

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("팔레트 파일 모드: Ctrl+P → 파일 검색 → Enter → 노트 로드", async () => {
  await loadVault(page, sampleVaultPath());

  // 팔레트 열기.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await expect(page.getByTestId("palette-input")).toBeVisible();

  // 파일명 일부 타이핑 — sample-vault의 실재 파일 "프로젝트".
  await page.getByTestId("palette-input").type("프로젝트");

  // 매칭 결과가 1개 이상 표시.
  await expect(page.getByTestId("palette-item").first()).toBeVisible();

  // Enter → 팔레트 닫히고 해당 노트가 에디터에 로드.
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

  // smoke.spec.ts의 에디터 검증 셀렉터 재사용.
  await expect(page.locator(".title")).toContainText("프로젝트");
  await expect(page.locator(".cm-content")).toBeVisible();
});

test("팔레트 명령 모드: Ctrl+P → '>테마' 입력 → Enter → 테마 전환", async () => {
  await loadVault(page, sampleVaultPath());

  const html = page.locator("html");
  const before = (await html.getAttribute("data-theme")) ?? "light";

  // 팔레트 열기.
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  // '>' 접두어로 명령 모드 전환 후 '테마' 입력 — "테마 전환(라이트↔다크)" fuzzy 매칭.
  await page.getByTestId("palette-input").type(">테마");

  // 명령 결과 표시.
  await expect(page.getByTestId("palette-item").first()).toBeVisible();

  // Enter → 팔레트 닫히고 테마 전환.
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

  // theme.spec.ts의 검증 방식 재사용: data-theme 속성이 반대 값으로 전환.
  await expect(html).toHaveAttribute("data-theme", opposite(before));
});

test("팔레트 Esc: 열린 상태에서 Esc → 오버레이 닫힘", async () => {
  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});
