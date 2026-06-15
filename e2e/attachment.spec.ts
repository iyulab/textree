import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  listVaultDir,
} from "./helpers";

let browser: Browser;
let page: Page;

// 1x1 투명 PNG.
const PNG_1x1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/**
 * 에디터에 이미지 paste 이벤트를 주입한다. Chromium은 ClipboardEvent 생성자의
 * clipboardData를 무시하므로 Object.defineProperty로 DataTransfer를 단다(앱의
 * handlePaste가 event.clipboardData.items를 읽는 계약과 일치).
 */
async function pasteImage(mime: string, base64: string): Promise<void> {
  await page.evaluate(
    ({ mime, base64 }) => {
      const bin = atob(base64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const file = new File([bytes], "clip", { type: mime });
      const dt = new DataTransfer();
      dt.items.add(file);
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", { value: dt });
      document.querySelector(".cm-content")!.dispatchEvent(ev);
    },
    { mime, base64 },
  );
}

test("이미지 붙여넣기 → assets/ 저장 + 본문에 링크 자동삽입", async () => {
  const vault = createTempVault({ "그림노트.md": "본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /그림노트/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");

    await pasteImage("image/png", PNG_1x1_BASE64);

    // 본문에 마크다운 링크가 삽입되고 자동저장으로 디스크 반영.
    await expect
      .poll(() => readVaultFile(vault, "그림노트.md"), { timeout: 5000 })
      .toContain("![](assets/");
    // 노트 옆 assets/에 실제 이미지 파일 저장.
    await expect
      .poll(() => listVaultDir(vault, "assets").filter((f) => f.endsWith(".png")).length, {
        timeout: 5000,
      })
      .toBeGreaterThan(0);
  } finally {
    removeTempVault(vault);
  }
});

test("미지원 형식(image/tiff) 붙여넣기는 가로채지 않음 — assets 미생성", async () => {
  const vault = createTempVault({ "무관노트.md": "본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /무관노트/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();

    // tiff는 화이트리스트 밖 → handlePaste가 통과시킴(삼킴 방지). 첨부 저장 없음.
    await pasteImage("image/tiff", PNG_1x1_BASE64);

    // 잠시 대기 후에도 assets 미생성·링크 미삽입 확인.
    await page.waitForTimeout(800);
    expect(listVaultDir(vault, "assets")).toHaveLength(0);
    expect(readVaultFile(vault, "무관노트.md")).not.toContain("![](assets/");
  } finally {
    removeTempVault(vault);
  }
});
