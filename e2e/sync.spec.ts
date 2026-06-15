import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  writeVaultFile,
} from "./helpers";
import { rmSync } from "node:fs";
import { join } from "node:path";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("외부 수정 → 깨끗한 노트는 디스크 내용으로 재로드", async () => {
  const vault = createTempVault({ "외부.md": "원래 내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /외부/ }).click();
    await expect(page.locator(".cm-content")).toContainText("원래 내용");

    // 외부 도구가 파일을 수정(앱이 안 쓴 고유 내용 → 에코 억제 미적용).
    writeVaultFile(vault, "외부.md", "외부에서 바뀐 내용 12345\n");

    await expect(page.locator(".cm-content")).toContainText("외부에서 바뀐 내용 12345");
  } finally {
    removeTempVault(vault);
  }
});

test("외부 생성 → 트리에 새 노드 등장", async () => {
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);
    await expect(page.getByRole("treeitem", { name: /기존/ })).toBeVisible();

    writeVaultFile(vault, "새로생긴노트.md", "새 노트\n");

    await expect(page.getByRole("treeitem", { name: /새로생긴노트/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("외부 삭제 → 열린 노트에 이동/삭제 표시", async () => {
  const vault = createTempVault({ "삭제대상.md": "곧 사라짐\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /삭제대상/ }).click();
    await expect(page.locator(".cm-content")).toContainText("곧 사라짐");

    rmSync(join(vault, "삭제대상.md"), { force: true });

    await expect(page.locator(".status.error")).toContainText("외부에서 이동/삭제됨");
  } finally {
    removeTempVault(vault);
  }
});

/**
 * 결정론적으로 충돌 상태를 만든다. 충돌은 본질적으로 타이밍 경쟁(자동저장 500ms
 * vs 워처 ~300ms)이라 단발로는 비결정적 — 매 폴링마다 (1) 키 입력으로 자동저장
 * 디바운스를 리셋해 dirty를 유지하고, (2) 새 외부 변경을 유발해 워처 이벤트가 항상
 * dirty 상태에서 도착하게 한다. 반환값은 디스크에 마지막으로 쓴 외부 내용.
 */
async function triggerConflict(vault: string): Promise<string> {
  await loadVault(page, vault);
  await page.getByRole("treeitem", { name: /충돌/ }).click();
  await expect(page.locator(".cm-content")).toBeVisible();
  await page.locator(".cm-content").click();
  await page.keyboard.press("Control+End");

  let i = 0;
  let lastExternal = "";
  const banner = page.locator(".banner");
  await expect(async () => {
    await page.keyboard.type("x");
    lastExternal = `외부가 덮어쓴 내용 ${i++}`;
    writeVaultFile(vault, "충돌.md", `${lastExternal}\n`);
    await expect(banner).toBeVisible({ timeout: 400 });
  }).toPass({ timeout: 8000 });
  return lastExternal;
}

test("미저장 편집 중 외부 수정 → 충돌 배너 + 해소 버튼", async () => {
  const vault = createTempVault({ "충돌.md": "초기\n" });
  try {
    await triggerConflict(vault);
    await expect(page.locator(".banner")).toContainText("외부에서 변경되었습니다");
    await expect(page.getByRole("button", { name: "디스크 버전 불러오기" })).toBeVisible();
    await expect(page.getByRole("button", { name: "내 편집 유지" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("충돌 해소: 디스크 버전 불러오기 → 에디터가 디스크 내용으로 교체", async () => {
  const vault = createTempVault({ "충돌.md": "초기\n" });
  try {
    const external = await triggerConflict(vault);
    await page.getByRole("button", { name: "디스크 버전 불러오기" }).click();

    await expect(page.locator(".banner")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText(external);
    // 내 편집("x")은 폐기됨.
    await expect(page.locator(".cm-content")).not.toContainText("초기x");
  } finally {
    removeTempVault(vault);
  }
});

test("충돌 해소: 내 편집 유지 → 배너 닫고 내 편집 보존(디스크 버전 미적용)", async () => {
  const vault = createTempVault({ "충돌.md": "초기\n" });
  try {
    await triggerConflict(vault);
    await page.getByRole("button", { name: "내 편집 유지" }).click();

    // "내 편집 유지"의 책임 = 배너를 닫고 내 편집을 폐기하지 않는 것.
    // (디스크 교체와 대조 — 디스크 버전 불러오기 테스트가 반대 경로를 검증.)
    // 디스크 영속 자체는 자동저장 메커니즘(editing.spec)에서 이미 검증되므로,
    // 여기서는 워처 잔여 이벤트와 경쟁하지 않도록 UI 상태에만 집중한다.
    await expect(page.locator(".banner")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText("초기");
    await expect(page.locator(".cm-content")).not.toContainText("외부가 덮어쓴");
  } finally {
    removeTempVault(vault);
  }
});
