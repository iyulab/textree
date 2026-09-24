import { test, expect, type Browser, type Page } from "@playwright/test";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

/**
 * Opening a folder the way an earlier release left it on disk.
 *
 * Until 0.7.0 the app kept its own files inside the notes folder, under `.textree/`: settings, a
 * trash of set-aside copies with a list describing them, and temp files. The first open in a
 * later version carries all of that out. Every shape an earlier release could leave belongs here,
 * because this is the first thing an update does to someone's real folder — and no other spec
 * starts from anything but a clean one.
 *
 * Shapes covered (each one a thing 0.6.0 itself treated as normal):
 *  - settings plus listed copies                    → everything carried, folder clean
 *  - a copy the list never named (unknown origin)   → carried to the top of the folder
 *  - a deleted folder holding a note and an image   → counted as a note and a file
 *  - a list that no longer parses                   → its copies still carried
 *  - something that cannot be carried               → stays, and the notice says so every time
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

const at = (iso: string) => Math.floor(Date.parse(iso) / 1000);

type Listed = { trashName: string; originalRel: string; deletedAt: number; isDir: boolean };

/** A folder as an earlier release left it. `copies` are files under `.textree/trash/`. */
function earlierRelease(copies: Record<string, string>, list: Listed[] | string, extra: Record<string, string> = {}) {
  const files: Record<string, string> = {
    "alpha.md": "# alpha\n",
    ".textree/favorites.json": '["alpha.md"]',
    ".textree/views.json": "[]",
    ".textree/tmp/.tmpA1b2": "stale",
    ".textree/trash.json": typeof list === "string" ? list : JSON.stringify(list),
    ...extra,
  };
  for (const [name, body] of Object.entries(copies)) files[`.textree/trash/${name}`] = body;
  return createTempVault(files);
}

/** Opens Deleted notes and waits until every name in `expected` is listed, then reads it. */
async function openDeletedNotes(expected: string[]): Promise<string> {
  await page.keyboard.press("Control+p");
  await page.getByTestId("palette-input").fill(">deleted");
  await expect(page.getByTestId("palette-item").first()).toContainText("Deleted notes");
  await page.keyboard.press("Enter");
  const panel = page.getByTestId("deleted-notes");
  // The list is read from the history after the panel opens.
  for (const name of expected) await expect(panel).toContainText(name);
  return panel.innerText();
}

test("upgrade: every copy an earlier release set aside reaches Deleted notes, and the folder is left clean", async () => {
  const vault = earlierRelease(
    {
      "old-idea.md": "# old idea\nonly copy\n",
      "later.md": "# later\n",
      "proj/plan.md": "# plan\n",
      "proj/diagram.png": "PNG",
      "stray.md": "# stray\nnamed by no list\n",
    },
    [
      { trashName: "old-idea.md", originalRel: "old-idea.md", deletedAt: at("2026-05-01T09:00:00Z"), isDir: false },
      { trashName: "later.md", originalRel: "later.md", deletedAt: at("2026-07-20T09:00:00Z"), isDir: false },
      { trashName: "proj", originalRel: "proj", deletedAt: at("2026-06-10T09:00:00Z"), isDir: true },
      { trashName: "gone.md", originalRel: "gone.md", deletedAt: at("2026-06-01T09:00:00Z"), isDir: false },
    ],
  );
  try {
    await loadVault(page, vault);

    const notice = page.getByTestId("migration-notice");
    await expect(notice).toContainText("Two things moved out of your notes folder");
    await expect(notice).toContainText("4 notes and 1 other file from the trash are now under Deleted notes.");
    await expect(notice).toContainText("Your folder now holds only your notes.");
    await page.getByTestId("migration-ok").click();

    expect(existsSync(join(vault, ".textree")), "nothing of the app's is left in the folder").toBe(false);
    expect(readdirSync(vault).sort()).toEqual([".git", "alpha.md"]);

    const listed = await openDeletedNotes(["old-idea", "later", "plan", "stray"]);
    // Ordered by when each left, the earlier release's own dates kept.
    expect(listed.indexOf("later")).toBeLessThan(listed.indexOf("old-idea"));
    await page.keyboard.press("Escape");

    // Opening it again says nothing: there is nothing left to say.
    await loadVault(page, createTempVault({ "other.md": "x\n" }));
    await loadVault(page, vault);
    await expect(page.getByTestId("migration-notice")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});

test("upgrade: a list that no longer parses does not hide the copies it described", async () => {
  const vault = earlierRelease({ "one.md": "# one\n", "two.md": "# two\n" }, "{not json");
  try {
    await loadVault(page, vault);
    await expect(page.getByTestId("migration-notice")).toContainText("2 notes from the trash are now under Deleted notes.");
    await page.getByTestId("migration-ok").click();
    expect(existsSync(join(vault, ".textree"))).toBe(false);
    await openDeletedNotes(["one", "two"]);
    await page.keyboard.press("Escape");
  } finally {
    removeTempVault(vault);
  }
});

test("upgrade: what cannot be carried stays untouched, and the notice never calls the folder clean", async () => {
  const vault = earlierRelease(
    { "one.md": "# one\n", ".odd": "a name no note can have" },
    [{ trashName: "one.md", originalRel: "one.md", deletedAt: at("2026-06-01T09:00:00Z"), isDir: false }],
  );
  try {
    await loadVault(page, vault);
    const notice = page.getByTestId("migration-notice");
    await expect(notice).toContainText("1 note from the trash is now under Deleted notes.");
    await expect(notice).toContainText(".textree/trash/.odd");
    await expect(notice).not.toContainText("only your notes");
    await page.getByTestId("migration-ok").click();

    // Opening it again carries nothing twice, and still says what stayed.
    await loadVault(page, createTempVault({ "other.md": "x\n" }));
    await loadVault(page, vault);
    await expect(notice).toContainText("Some files stayed in your notes folder");
    await expect(notice).not.toContainText("from the trash");
    await page.getByTestId("migration-ok").click();

    const listed = await openDeletedNotes(["one"]);
    expect(listed.match(/\bone\b/g)?.length ?? 0, "listed once").toBe(1);
    await page.keyboard.press("Escape");
    expect(existsSync(join(vault, ".textree", "trash", ".odd"))).toBe(true);
  } finally {
    removeTempVault(vault);
  }
});
