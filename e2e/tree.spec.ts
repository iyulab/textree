import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

/**
 * D3 — 트리 펼침/접힘. 컨테이너의 chevron 으로 자식 표시를 토글하고 상태가
 * localStorage 에 영속하는지 실제 webview 에서 단언한다.
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("컨테이너 chevron 으로 자식 펼침/접힘 + 영속", async () => {
  const vault = createTempVault({
    "폴더/자식1.md": "a\n",
    "폴더/자식2.md": "b\n",
  });
  try {
    // 트리 접힘 상태가 직전 테스트로 오염되지 않도록 초기화.
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    // 기본: 펼침 → 자식 보임.
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();
    await expect(page.getByRole("treeitem", { name: /자식2/ })).toBeVisible();

    // 접기 → 자식 사라짐. (exact: 사이드바 접기 버튼과 구분)
    await page.getByRole("button", { name: "접기", exact: true }).click();
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /자식2/ })).toHaveCount(0);

    // 접힘 상태가 영속(경로 1개 저장).
    const stored = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("textree-tree-collapsed") ?? "[]"),
    );
    expect(stored.length).toBe(1);

    // 펼치기 → 자식 복귀.
    await page.getByRole("button", { name: "펼치기", exact: true }).click();
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("키보드: ↑↓ 이동 · ←→ 접기/펼치기 · Enter 열기", async () => {
  const vault = createTempVault({
    "폴더/자식1.md": "a\n",
    "폴더/자식2.md": "b\n",
  });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    const folder = page.getByRole("treeitem", { name: /폴더/ });
    await folder.focus();

    // ← 접기: 자식 숨김.
    await page.keyboard.press("ArrowLeft");
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toHaveCount(0);

    // → 펼치기: 자식 복귀.
    await page.keyboard.press("ArrowRight");
    await expect(page.getByRole("treeitem", { name: /자식1/ })).toBeVisible();

    // ↓ 이동: 포커스가 다음 항목(자식1)으로.
    await page.keyboard.press("ArrowDown");
    const focusedText = await page.evaluate(
      () => document.activeElement?.textContent ?? "",
    );
    expect(focusedText).toContain("자식1");

    // Enter: 포커스 노트 열기.
    await page.keyboard.press("Enter");
    await expect(page.locator(".note-name")).toHaveText("자식1");
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("키보드: F2 이름변경 모드 진입 · Delete 삭제", async () => {
  const vault = createTempVault({ "지울노트.md": "x\n", "남길노트.md": "y\n" });
  try {
    await loadVault(page, vault);

    // F2 → 이름변경 입력창 등장(선택+모드).
    await page.getByRole("treeitem", { name: /지울노트/ }).focus();
    await page.keyboard.press("F2");
    await expect(page.locator(".name-input")).toBeVisible();
    await page.keyboard.press("Escape");

    // Delete → 휴지통으로(트리에서 사라짐).
    await page.getByRole("treeitem", { name: /지울노트/ }).focus();
    await page.keyboard.press("Delete");
    await expect(page.getByRole("treeitem", { name: /지울노트/ })).toHaveCount(0);
    await expect(page.getByRole("treeitem", { name: /남길노트/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("키보드 이동: Ctrl+X 잘라내기 → 폴더에서 Ctrl+V 이동", async () => {
  const vault = createTempVault({
    "이동노트.md": "a\n",
    "대상폴더/기존.md": "b\n",
  });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /이동노트/ }).focus();
    await page.keyboard.press("Control+x");
    await page.getByRole("treeitem", { name: /대상폴더/ }).focus();
    await page.keyboard.press("Control+v");

    // 디스크: 대상폴더 안으로 이동.
    await expect
      .poll(() => existsSync(join(vault, "대상폴더", "이동노트.md")))
      .toBe(true);
    expect(existsSync(join(vault, "이동노트.md"))).toBe(false);
  } finally {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    removeTempVault(vault);
  }
});

test("키보드 adopt: 리프에서 Ctrl+V → 승격 후 자식", async () => {
  const vault = createTempVault({ "소스.md": "a\n", "리프.md": "b\n" });
  try {
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /소스/ }).focus();
    await page.keyboard.press("Control+x");
    await page.getByRole("treeitem", { name: /리프/ }).focus();
    await page.keyboard.press("Control+v");

    // 리프가 컨테이너로 승격되고 소스가 그 안으로(adopt).
    await expect.poll(() => existsSync(join(vault, "리프", "소스.md"))).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("브레드크럼: 중첩 노트의 조상 폴더 표시", async () => {
  const vault = createTempVault({ "폴더/자식1.md": "a\n" });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);

    await page.getByRole("treeitem", { name: /자식1/ }).click();
    // 본문 헤더에 조상 폴더(폴더) + 노트명(자식1).
    await expect(page.locator(".crumb")).toHaveText("폴더");
    await expect(page.locator(".note-name")).toHaveText("자식1");
  } finally {
    removeTempVault(vault);
  }
});
