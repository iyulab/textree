import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  writeVaultFile,
  readVaultFile,
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

test("external modify → clean note reloads from disk content", async () => {
  const vault = createTempVault({ "external.md": "original content\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /external/ }).click();
    await expect(page.locator(".cm-content")).toContainText("original content");

    // An external tool modifies the file (unique content the app never wrote → echo suppression does not apply).
    writeVaultFile(vault, "external.md", "changed outside 12345\n");

    await expect(page.locator(".cm-content")).toContainText("changed outside 12345");
  } finally {
    removeTempVault(vault);
  }
});

test("external create → new node appears in the tree", async () => {
  const vault = createTempVault({ "existing.md": "x\n" });
  try {
    await loadVault(page, vault);
    await expect(page.getByRole("treeitem", { name: /existing/ })).toBeVisible();

    writeVaultFile(vault, "appeared-note.md", "new note\n");

    await expect(page.getByRole("treeitem", { name: /appeared-note/ })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("external delete → open note shows moved/deleted indicator", async () => {
  const vault = createTempVault({ "delete-target.md": "about to vanish\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /delete-target/ }).click();
    await expect(page.locator(".cm-content")).toContainText("about to vanish");

    rmSync(join(vault, "delete-target.md"), { force: true });

    await expect(page.locator(".status.error")).toContainText("Moved/deleted externally");
  } finally {
    removeTempVault(vault);
  }
});

/**
 * Deterministically produce a conflict state. A conflict is inherently a timing race
 * (autosave 500ms vs watcher ~300ms), so a single attempt is non-deterministic — on each
 * poll we (1) reset the autosave debounce with a keystroke to keep it dirty, and
 * (2) trigger a new external change so the watcher event always arrives while dirty.
 * Returns the last external content written to disk.
 */
async function triggerConflict(vault: string): Promise<string> {
  await loadVault(page, vault);
  await page.getByRole("treeitem", { name: /clash/ }).click();
  await expect(page.locator(".cm-content")).toBeVisible();
  await page.locator(".cm-content").click();
  await page.keyboard.press("Control+End");

  let i = 0;
  let lastExternal = "";
  const banner = page.locator(".banner");
  await expect(async () => {
    await page.keyboard.type("x");
    lastExternal = `overwritten outside ${i++}`;
    writeVaultFile(vault, "clash.md", `${lastExternal}\n`);
    await expect(banner).toBeVisible({ timeout: 400 });
  }).toPass({ timeout: 8000 });
  return lastExternal;
}

test("external modify during unsaved edit → conflict banner + resolution buttons", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    await expect(page.locator(".banner")).toContainText("changed on disk");
    await expect(page.getByRole("button", { name: "Load the copy on disk" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Keep my edits" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("conflict resolution: load the copy on disk → editor replaced with disk content", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    await page.getByRole("button", { name: "Load the copy on disk" }).click();

    await expect(page.locator(".banner")).toHaveCount(0);
    // The editor ends up showing what is on disk. Not a specific write: the retry loop that raised
    // the conflict can land one more external write after the one that raised the banner, and
    // either is a correct "copy on disk" — what must not happen is the editor settling on anything
    // other than the file's current content.
    await expect
      .poll(
        async () => {
          const shown = (await page.locator(".cm-content").innerText()).trim();
          return shown === readVaultFile(vault, "clash.md").trim() ? "matches disk" : shown;
        },
        { timeout: 5000 },
      )
      .toBe("matches disk");
    await expect(page.locator(".cm-content")).toContainText("overwritten outside");
    // My edit ("x") is discarded.
    await expect(page.locator(".cm-content")).not.toContainText("initialx");
  } finally {
    removeTempVault(vault);
  }
});

test("conflict pending: further typing is not saved over the copy on disk until I choose", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    const onDisk = readVaultFile(vault, "clash.md");

    // Keep typing with the question still open, and wait well past the autosave debounce.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("more of mine");
    await page.waitForTimeout(1500);

    // The copy on disk is untouched: the timer must not answer the question for the user.
    expect(readVaultFile(vault, "clash.md")).toBe(onDisk);
    await expect(page.locator(".banner")).toBeVisible();

    // Choosing to keep my edits is what replaces it.
    await page.getByRole("button", { name: "Keep my edits" }).click();
    await expect.poll(() => readVaultFile(vault, "clash.md"), { timeout: 5000 }).toContain("more of mine");
  } finally {
    removeTempVault(vault);
  }
});

test("conflict resolution: keep my edit → close banner and preserve my edit (disk version not applied)", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    await page.getByRole("button", { name: "Keep my edits" }).click();

    // "keep my edit" responsibility = close the banner and not discard my edit.
    // (Contrast with disk replacement — the load-disk-version test verifies the opposite path.)
    // Disk persistence itself is already verified by the autosave mechanism (editing.spec),
    // so here we focus only on UI state to avoid racing with residual watcher events.
    await expect(page.locator(".banner")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText("initial");
    await expect(page.locator(".cm-content")).not.toContainText("overwritten outside");
  } finally {
    removeTempVault(vault);
  }
});
