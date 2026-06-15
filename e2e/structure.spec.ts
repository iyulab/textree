import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  dragNodeOnto,
} from "./helpers";
import { existsSync } from "node:fs";
import { join } from "node:path";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

const exists = (vault: string, rel: string) => existsSync(join(vault, rel));

test("＋노트 → 디스크에 .md 생성 + 트리 등장", async () => {
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("button", { name: "＋노트" }).click();
    await page.locator(".name-input").fill("새노트");
    await page.locator(".name-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새노트/ })).toBeVisible();
    expect(exists(vault, "새노트.md")).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("＋폴더 → 디스크에 디렉터리 생성", async () => {
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("button", { name: "＋폴더" }).click();
    await page.locator(".name-input").fill("새폴더");
    await page.locator(".name-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새폴더/ })).toBeVisible();
    expect(exists(vault, "새폴더")).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("이름변경 → 디스크 파일명 변경", async () => {
  const vault = createTempVault({ "옛이름.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /옛이름/ }).click();
    await page.getByRole("button", { name: "이름변경", exact: true }).click();
    await page.locator(".name-input").fill("새이름");
    await page.locator(".name-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새이름/ })).toBeVisible();
    expect(exists(vault, "새이름.md")).toBe(true);
    expect(exists(vault, "옛이름.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("삭제 → 휴지통 이동(원본 사라짐)", async () => {
  const vault = createTempVault({ "삭제할노트.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /삭제할노트/ }).click();
    await page.getByRole("button", { name: "삭제", exact: true }).click();

    await expect(page.getByRole("treeitem", { name: /삭제할노트/ })).toHaveCount(0);
    expect(exists(vault, "삭제할노트.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("볼트 전환 후 생성은 새 볼트를 타깃 — stale 선택 격리(회귀)", async () => {
  // 회귀: loadVault가 이전 볼트의 selectedNode를 정리하지 않으면, ＋노트의
  // targetParent가 이전 볼트 경로를 가리켜 엉뚱한 곳에 생성된다.
  const v1 = createTempVault({ "v1노트.md": "1\n" });
  const v2 = createTempVault({ "v2노트.md": "2\n" });
  try {
    await loadVault(page, v1);
    await page.getByRole("treeitem", { name: /v1노트/ }).click(); // selectedNode 설정
    await expect(page.locator(".cm-content")).toBeVisible();

    await loadVault(page, v2); // 볼트 전환 — 이전 선택은 무효여야 함
    await page.getByRole("button", { name: "＋노트" }).click();
    await page.locator(".name-input").fill("새것");
    await page.locator(".name-input").press("Enter");

    await expect.poll(() => exists(v2, "새것.md"), { timeout: 5000 }).toBe(true);
    expect(exists(v1, "새것.md")).toBe(false);
  } finally {
    removeTempVault(v1);
    removeTempVault(v2);
  }
});

test("DnD 노트 → 폴더로 이동", async () => {
  const vault = createTempVault({
    "이동노트.md": "옮길 내용\n",
    "대상폴더/대상폴더.md": "폴더 본문\n",
  });
  try {
    await loadVault(page, vault);
    const src = page.getByRole("treeitem", { name: /이동노트/ });
    const dst = page.getByRole("treeitem", { name: /대상폴더/ });
    await expect(src).toBeVisible();
    await dragNodeOnto(page, src, dst);

    await expect.poll(() => exists(vault, "대상폴더/이동노트.md"), { timeout: 5000 }).toBe(true);
    expect(exists(vault, "이동노트.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("＋하위 → 리프 승격 후 자식 노트 생성", async () => {
  const vault = createTempVault({ "부모.md": "부모 본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /부모/ }).click();
    await page.getByRole("button", { name: "＋하위" }).click();
    await page.locator(".name-input").fill("자식");
    await page.locator(".name-input").press("Enter");

    // 승격: 부모.md → 부모/부모.md, 그리고 부모/자식.md 생성.
    await expect.poll(() => exists(vault, "부모/부모.md"), { timeout: 5000 }).toBe(true);
    expect(exists(vault, "부모/자식.md")).toBe(true);
    expect(exists(vault, "부모.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("DnD 리프 위로 드롭 → adopt(승격 후 자식)", async () => {
  const vault = createTempVault({
    "드롭소스.md": "소스 내용\n",
    "드롭대상.md": "대상 내용\n",
  });
  try {
    await loadVault(page, vault);
    const src = page.getByRole("treeitem", { name: /드롭소스/ });
    const dst = page.getByRole("treeitem", { name: /드롭대상/ });
    await expect(src).toBeVisible();
    await dragNodeOnto(page, src, dst);

    // 드롭대상이 컨테이너로 승격되고 드롭소스가 그 자식으로.
    await expect.poll(() => exists(vault, "드롭대상/드롭대상.md"), { timeout: 5000 }).toBe(true);
    expect(exists(vault, "드롭대상/드롭소스.md")).toBe(true);
    expect(exists(vault, "드롭소스.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});
