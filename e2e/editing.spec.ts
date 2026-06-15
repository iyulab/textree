import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
} from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("편집 → 디바운스 자동저장 → 디스크에 반영", async () => {
  const vault = createTempVault({ "메모.md": "처음 내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /메모/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    // 본문 끝에 한 줄 추가 입력.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("추가된 줄");

    // 디바운스(500ms) 후 디스크에 기록될 때까지 폴링 — 실제 fs 결과를 직접 검증.
    await expect
      .poll(() => readVaultFile(vault, "메모.md"), { timeout: 5000 })
      .toContain("추가된 줄");
  } finally {
    removeTempVault(vault);
  }
});

test("노트 전환 시 미저장 편집을 flush", async () => {
  const vault = createTempVault({ "노트가.md": "가본문\n", "노트나.md": "나본문\n" });
  try {
    await loadVault(page, vault);

    // 노트가 편집(디바운스 만료 전에 곧바로 노트나로 전환).
    await page.getByRole("treeitem", { name: /노트가/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("XYZ");

    // 즉시 노트나로 전환 → flush가 노트가의 편집을 디스크에 보존해야 함.
    await page.getByRole("treeitem", { name: /노트나/ }).click();

    await expect
      .poll(() => readVaultFile(vault, "노트가.md"), { timeout: 5000 })
      .toContain("XYZ");
  } finally {
    removeTempVault(vault);
  }
});
