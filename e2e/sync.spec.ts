import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  writeVaultFile,
  readVaultFile,
  listVaultDir,
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
    // Nothing was unsaved, so there is nothing to ask about.
    await expect(page.getByTestId("removed-banner")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});

/** Open `gone.md`, type unsaved edits, then delete the file outside the app before they are saved. */
async function deleteWhileEditing(vault: string): Promise<void> {
  await loadVault(page, vault);
  await page.getByRole("treeitem", { name: /gone/ }).click();
  await expect(page.locator(".cm-content")).toContainText("before");
  await page.locator(".cm-content").click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type(" typed after");
  // Well inside the 500 ms save debounce: whether the watcher or the save notices first, the
  // edits were never written anywhere.
  rmSync(join(vault, "gone.md"), { force: true });
  await expect(page.getByTestId("removed-banner")).toBeVisible();
}

test("deleted outside while editing: the unsaved edits stay and can be put back", async () => {
  const vault = createTempVault({ "gone.md": "before\n", "other.md": "# other\n" });
  try {
    await deleteWhileEditing(vault);
    await expect(page.locator(".cm-content")).toContainText("before typed after");

    await page.getByRole("button", { name: "Put it back with my edits" }).click();

    await expect(page.getByTestId("removed-banner")).toHaveCount(0);
    await expect.poll(() => readVaultFile(vault, "gone.md")).toMatch(/^before\s*typed after/);
    // Back to an ordinary note: further typing is saved into it.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type(" again");
    await expect.poll(() => readVaultFile(vault, "gone.md")).toMatch(/typed after again/);
  } finally {
    removeTempVault(vault);
  }
});

test("deleted outside with its folder while editing: the note comes back at the top", async () => {
  const vault = createTempVault({ "box/gone.md": "before\n", "other.md": "# other\n" });
  try {
    await page.evaluate(() => localStorage.removeItem("textree-tree-collapsed"));
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /gone/ }).click();
    await expect(page.locator(".cm-content")).toContainText("before");
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type(" typed after");
    rmSync(join(vault, "box"), { recursive: true, force: true });
    await expect(page.getByTestId("removed-banner")).toBeVisible();

    await page.getByRole("button", { name: "Put it back with my edits" }).click();

    await expect(page.getByTestId("removed-banner")).toHaveCount(0);
    await expect.poll(() => readVaultFile(vault, "gone.md")).toMatch(/^before\s*typed after/);
  } finally {
    removeTempVault(vault);
  }
});

test("deleted outside while editing: leaving is refused until I choose, then discarding lets go", async () => {
  const vault = createTempVault({ "gone.md": "before\n", "other.md": "# other\n" });
  try {
    await deleteWhileEditing(vault);

    await page.getByRole("treeitem", { name: /other/ }).click();

    await expect(page.getByTestId("removed-banner")).toHaveClass(/attention/);
    await expect
      .poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.testid))
      .toBe("removed-banner");
    await expect(page.locator(".cm-content")).toContainText("before typed after");

    await page.getByRole("button", { name: "Discard my edits" }).click();
    await expect(page.getByText("This note was moved or deleted externally. Select another note.")).toBeVisible();
    expect(listVaultDir(vault, ".").filter((f) => f.startsWith("gone"))).toEqual([]);

    await page.getByRole("treeitem", { name: /other/ }).click();
    await expect(page.locator(".cm-content")).toContainText("other");
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

/**
 * Answer "Keep my edits" until the app stops asking. The loop in triggerConflict can land one more
 * external write after the banner showed the previous one; keeping mine would then replace a copy
 * the person never saw, so the app asks again (by design) — and the answer is given again.
 */
async function keepMineUntilSaved(vault: string, mine: RegExp): Promise<void> {
  // Judged by the file, not the banner: between one answer and the app asking again the banner is
  // briefly gone, and reading that as "settled" would stop answering too early.
  const banner = page.getByTestId("conflict-banner");
  await expect(async () => {
    if (await banner.isVisible()) await page.getByRole("button", { name: "Keep my edits" }).click();
    expect(readVaultFile(vault, "clash.md")).toMatch(mine);
  }).toPass({ timeout: 5000 });
  await expect(banner).toHaveCount(0);
}

test("external modify during unsaved edit → conflict banner + resolution buttons", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    await expect(page.locator(".banner")).toContainText("changed on disk");
    await expect(page.getByRole("button", { name: "Load the copy on disk" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Keep my edits" })).toBeVisible();
    // Answer it: the app will not leave this note while the question is open.
    await page.getByRole("button", { name: "Load the copy on disk" }).click();
  } finally {
    removeTempVault(vault);
  }
});

test("conflict pending: leaving the note is refused until I choose, and nothing is lost", async () => {
  const vault = createTempVault({ "clash.md": "initial\n", "other.md": "# other\n" });
  try {
    await triggerConflict(vault);
    const onDisk = readVaultFile(vault, "clash.md");

    // Try to open another note with the question unanswered.
    await page.getByRole("treeitem", { name: /other/ }).click();

    // Still here, my edits still on screen, the copy on disk untouched, and the question pressed.
    await expect(page.getByTestId("conflict-banner")).toHaveClass(/attention/);
    // Focus goes to the question, not to an answer: a stray Enter must not discard my edits.
    // (Read from the document: `toBeFocused` also needs the OS window to be foreground.)
    await expect
      .poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.testid))
      .toBe("conflict-banner");
    await expect(page.locator(".cm-content")).toContainText("initialx");
    expect(readVaultFile(vault, "clash.md")).toBe(onDisk);

    // Once answered, leaving works — and what I kept is what the note holds.
    await keepMineUntilSaved(vault, /^initial\s*x/);
    await page.getByRole("treeitem", { name: /other/ }).click();
    await expect(page.locator(".cm-content")).toContainText("other");
  } finally {
    removeTempVault(vault);
  }
});

test("a save never replaces a change it has not seen", async () => {
  const vault = createTempVault({ "cas.md": "loaded\n" });
  try {
    await loadVault(page, vault);
    writeVaultFile(vault, "cas.md", "changed elsewhere\n");
    const invoke = (content: string, expected: string) =>
      page.evaluate(
        ([root, content, expected]) =>
          (
            window as unknown as {
              __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<unknown> };
            }
          ).__TAURI_INTERNALS__.invoke("write_note", { root, path: `${root}/cas.md`, content, expected }),
        [vault, content, expected],
      );

    // Based on what was loaded, but the note has moved on: nothing is written.
    expect(await invoke("mine\n", "loaded\n")).toEqual({ kind: "conflict", disk: "changed elsewhere\n" });
    expect(readVaultFile(vault, "cas.md")).toBe("changed elsewhere\n");

    // Based on what is there now: written.
    expect(await invoke("mine\n", "changed elsewhere\n")).toEqual({ kind: "written" });
    expect(readVaultFile(vault, "cas.md")).toBe("mine\n");
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
    await keepMineUntilSaved(vault, /more of mine/);
  } finally {
    removeTempVault(vault);
  }
});

test("conflict resolution: keep my edit → close banner and preserve my edit (disk version not applied)", async () => {
  const vault = createTempVault({ "clash.md": "initial\n" });
  try {
    await triggerConflict(vault);
    await keepMineUntilSaved(vault, /^initial\s*x/);

    // "keep my edit" = my edit is what the note holds (checked above), the banner closes, and the
    // editor still shows mine. (The load-disk-version test verifies the opposite path.)
    await expect(page.locator(".banner")).toHaveCount(0);
    await expect(page.locator(".cm-content")).toContainText("initial");
    await expect(page.locator(".cm-content")).not.toContainText("overwritten outside");
  } finally {
    removeTempVault(vault);
  }
});
