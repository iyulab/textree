import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * P1b — 본문 전문검색. Ctrl+P로 팔레트를 열고 '/' 접두어(본문검색 모드)로 질의해
 * 실제 tantivy 인덱스(앱데이터)·백엔드 IPC를 거쳐 본문 매치 + 스니펫을 단언한다.
 *
 * 격리 임시 볼트를 쓴다(다른 구조/편집 스펙과 동일 패턴). 본문에만 존재하는
 * 고유 토큰을 심어, 파일명 fuzzy가 아니라 본문 인덱스가 매치함을 보장한다.
 *
 * 주의: open_vault는 인덱스를 백그라운드 스레드에서 빌드한다. 첫 질의 시점에
 * 인덱스가 비어 있을 수 있으므로 Playwright의 자동 재시도 expect(timeout)로
 * 빌드 완료까지 자연 대기한다.
 */

let browser: Browser;
let page: Page;

// 본문에만 등장하는 고유 토큰(파일명·제목에는 없음 → 본문 인덱스만 매치).
const TOKEN = "전문검색고유토큰";

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("본문 전문검색: '/' 모드로 본문 매치 + 스니펫", async () => {
  const vault = createTempVault({
    "메모.md": `# 메모\n\n오늘 ${TOKEN} 기능을 설계했다.\n`,
    "딴노트.md": "# 딴노트\n\n관계없는 본문.\n",
  });
  try {
    await loadVault(page, vault);

    // 팔레트 열기.
    await page.keyboard.press("Control+p");
    const input = page.getByTestId("palette-input");
    await expect(input).toBeVisible();

    // '/' 접두어로 본문검색 모드 전환 후 고유 토큰 질의.
    await input.fill(`/${TOKEN}`);

    // 인덱스 백그라운드 빌드 — 결과 등장까지 자동 재시도 대기.
    const items = page.getByTestId("palette-item");
    await expect(items.first()).toBeVisible({ timeout: 10_000 });

    // 본문 매치는 1건(메모.md)만, 스니펫에 질의 토큰 포함.
    await expect(items).toHaveCount(1);
    await expect(items.first()).toContainText(TOKEN);

    // Enter → 팔레트 닫히고 해당 노트가 에디터에 로드.
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
    await expect(page.locator(".title")).toContainText("메모");
  } finally {
    removeTempVault(vault);
  }
});
