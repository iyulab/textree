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

test("＋note → create .md on disk + appears in tree", async () => {
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("button", { name: "New note" }).click();
    await page.locator(".name-input").fill("새노트");
    await page.locator(".name-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새노트/ })).toBeVisible();
    expect(exists(vault, "새노트.md")).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("＋folder → create directory on disk", async () => {
  const vault = createTempVault({ "기존.md": "x\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("button", { name: "New folder" }).click();
    await page.locator(".name-input").fill("새폴더");
    await page.locator(".name-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새폴더/ })).toBeVisible();
    expect(exists(vault, "새폴더")).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("rename → inline rename file on disk", async () => {
  const vault = createTempVault({ "옛이름.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /옛이름/ }).click();
    await page.getByRole("button", { name: "Rename", exact: true }).click();
    await page.locator(".tree-rename-input").fill("새이름");
    await page.locator(".tree-rename-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새이름/ })).toBeVisible();
    expect(exists(vault, "새이름.md")).toBe(true);
    expect(exists(vault, "옛이름.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("rename → illegal name keeps the input open with an inline error", async () => {
  const vault = createTempVault({ "정상.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /정상/ }).click();
    await page.getByRole("button", { name: "Rename", exact: true }).click();
    // A name containing a path separator is rejected by is_valid_name.
    await page.locator(".tree-rename-input").fill("a/b");
    await page.locator(".tree-rename-input").press("Enter");

    // Input stays open, an inline error shows, and the file is NOT renamed.
    await expect(page.locator(".tree-rename-input")).toBeVisible();
    await expect(page.locator(".tree-rename-error")).toBeVisible();
    expect(exists(vault, "정상.md")).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});

test("rename → inline rename folder (directory + folder note on disk)", async () => {
  const vault = createTempVault({ "옛폴더/옛폴더.md": "노트\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /옛폴더/ }).click();
    await page.getByRole("button", { name: "Rename", exact: true }).click();
    await page.locator(".tree-rename-input").fill("새폴더");
    await page.locator(".tree-rename-input").press("Enter");

    await expect(page.getByRole("treeitem", { name: /새폴더/ })).toBeVisible();
    expect(exists(vault, "새폴더")).toBe(true);
    expect(exists(vault, "옛폴더")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("delete → move to trash (original disappears)", async () => {
  const vault = createTempVault({ "삭제할노트.md": "내용\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /삭제할노트/ }).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();

    await expect(page.getByRole("treeitem", { name: /삭제할노트/ })).toHaveCount(0);
    expect(exists(vault, "삭제할노트.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("create after vault switch targets new vault — stale selection isolation (regression)", async () => {
  // Regression: if loadVault does not clear the previous vault's selectedNode,
  // ＋note's targetParent points at the previous vault path and creates in the wrong place.
  const v1 = createTempVault({ "v1노트.md": "1\n" });
  const v2 = createTempVault({ "v2노트.md": "2\n" });
  try {
    await loadVault(page, v1);
    await page.getByRole("treeitem", { name: /v1노트/ }).click(); // set selectedNode
    await expect(page.locator(".cm-content")).toBeVisible();

    await loadVault(page, v2); // vault switch — previous selection must be invalidated
    await page.getByRole("button", { name: "New note" }).click();
    await page.locator(".name-input").fill("새것");
    await page.locator(".name-input").press("Enter");

    await expect.poll(() => exists(v2, "새것.md"), { timeout: 5000 }).toBe(true);
    expect(exists(v1, "새것.md")).toBe(false);
  } finally {
    removeTempVault(v1);
    removeTempVault(v2);
  }
});

test("DnD note → move into folder", async () => {
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

test("＋child → promote leaf then create child note", async () => {
  const vault = createTempVault({ "부모.md": "부모 본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /부모/ }).click();
    await page.getByRole("button", { name: "Add child note" }).click();
    await page.locator(".name-input").fill("자식");
    await page.locator(".name-input").press("Enter");

    // Promote: 부모.md → 부모/부모.md, and create 부모/자식.md.
    await expect.poll(() => exists(vault, "부모/부모.md"), { timeout: 5000 }).toBe(true);
    expect(exists(vault, "부모/자식.md")).toBe(true);
    expect(exists(vault, "부모.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});

test("DnD drop onto leaf → adopt (promote then child)", async () => {
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

    // 드롭대상 is promoted to a container and 드롭소스 becomes its child.
    await expect.poll(() => exists(vault, "드롭대상/드롭대상.md"), { timeout: 5000 }).toBe(true);
    expect(exists(vault, "드롭대상/드롭소스.md")).toBe(true);
    expect(exists(vault, "드롭소스.md")).toBe(false);
  } finally {
    removeTempVault(vault);
  }
});
