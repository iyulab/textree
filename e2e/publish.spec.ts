import { test, expect, type Browser, type Page } from "@playwright/test";
import { mkdtempSync, existsSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  recordNotes,
  choosePublish,
} from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/**
 * Start the publish flow through the dev bridge (bypasses the native folder picker, mirroring
 * loadVault). The returned promise settles once the publish ends or is declined in the
 * confirmation. Requires the app to be launched with TEXTREE_CANOPY_CLI pointing at canopy's CLI.
 */
function startPublish(p: Page, out: string): Promise<void> {
  return p.evaluate(
    (o) =>
      (
        window as unknown as { __textreeTest: { publishTo: (o: string) => Promise<void> } }
      ).__textreeTest.publishTo(o),
    out,
  );
}

/** Answer "Publish" in the confirmation once it has worked out what goes out. */
async function confirmPublish(p: Page): Promise<void> {
  const go = p.getByTestId("publish-confirm-go");
  await expect(go).toBeEnabled({ timeout: 10000 });
  await go.click();
  await expect(p.getByTestId("publish-confirm")).toHaveCount(0);
}

/** Publish into `out`, confirming, and wait until it is over. */
async function publishTo(p: Page, out: string): Promise<void> {
  const publishing = startPublish(p, out);
  await confirmPublish(p);
  await publishing;
}

test("publish renders the vault to an auto-theming static site, source untouched", async () => {
  const source = "# Hello\r\n\r\nworld\r\n"; // CRLF: the source must come back byte-identical
  const vault = createTempVault({ "note.md": source });
  const out = mkdtempSync(join(tmpdir(), "textree-pub-")).replace(/\\/g, "/");
  try {
    await loadVault(page, vault);
    await recordNotes(page, vault, ["note.md"]);
    await choosePublish(page, vault, "", true);
    const publishing = startPublish(page, out);
    await confirmPublish(page);
    // While the renderer runs, the screen says so (the first render after install can take a while).
    await expect(page.locator(".publish-banner.publishing")).toContainText("Publishing…");
    await publishing;
    await expect(page.locator(".publish-banner.publishing")).toHaveCount(0);

    // canopy emitted the page.
    await expect
      .poll(() => existsSync(join(out, "note.html")), { timeout: 15000 })
      .toBe(true);

    // The app's tokens ride along as a stylesheet linked after the renderer's own, so they win, and
    // their dark values apply both by the system's preference (unless the reader switched the page
    // to light) and on a page switched to dark with its toggle.
    const tokens = readFileSync(join(out, "assets", "stylesheet-1.css"), "utf8");
    const mediaAt = tokens.indexOf("@media (prefers-color-scheme: dark)");
    expect(mediaAt).toBeGreaterThanOrEqual(0);
    expect(tokens.slice(mediaAt)).toContain(':root:not([data-theme="light"]) {');
    expect(tokens).toContain('[data-theme="dark"]');
    const html = readFileSync(join(out, "note.html"), "utf8");
    expect(html.indexOf("assets/stylesheet-1.css")).toBeGreaterThan(html.indexOf("tokens.css"));

    // Publishing is read-only over the source — the note (CRLF included) is byte-unchanged.
    expect(readVaultFile(vault, "note.md")).toBe(source);

    // The UI surfaced the success notice with self-host guidance (vault-level, no note open).
    await expect(page.locator(".publish-banner.ok")).toContainText("Published");
  } finally {
    removeTempVault(vault);
    removeTempVault(out);
  }
});

test("publish into the vault is rejected with friendly, actionable guidance", async () => {
  // Publishing into the vault itself violates the read-only-outward boundary. canopy is
  // resolved before the boundary check, so this (like the test above) needs TEXTREE_CANOPY_CLI.
  // The raw backend error ("the output directory must be outside the vault") is rewritten by
  // friendlyError to actionable guidance — verifies the domain string matches the mapping key.
  const vault = createTempVault({ "note.md": "# Hi\n" });
  try {
    await loadVault(page, vault);
    await recordNotes(page, vault, ["note.md"]);
    await choosePublish(page, vault, "", true);
    await publishTo(page, vault);
    await expect(page.locator(".publish-banner.error")).toContainText("outside your vault");
  } finally {
    removeTempVault(vault);
  }
});

test("the confirmation says what goes out, and Cancel publishes nothing", async () => {
  const vault = createTempVault({
    "kept.md": "# kept\n\nrecorded as it is\n",
    "draft.md": "# draft\n\nnever recorded\n",
    "pic.png": "not really a picture",
    ".env": "SECRET=1\n",
  });
  const out = mkdtempSync(join(tmpdir(), "textree-pub-")).replace(/\\/g, "/");
  try {
    await loadVault(page, vault);
    // Give one note a version and choose the whole folder: the other has no version to go out.
    await recordNotes(page, vault, ["kept.md"]);
    await choosePublish(page, vault, "", true);

    // Whatever an earlier publish left on the banner stays as it was: declining changes nothing.
    const bannerBefore = await page.locator(".publish-banner").allTextContents();
    const publishing = startPublish(page, out);
    const dialog = page.getByRole("dialog", { name: "Publish this folder?" });
    await expect(dialog).toBeVisible();
    await expect(page.getByTestId("publish-confirm-summary")).toContainText(
      "1 note in this folder will be published",
    );
    const neverRecorded = page.getByTestId("publish-confirm-never-recorded");
    await expect(neverRecorded).toContainText("draft");
    await expect(neverRecorded).not.toContainText("kept");
    await expect(page.getByTestId("publish-confirm-go")).toBeFocused();

    await page.getByTestId("publish-confirm-cancel").click();
    await publishing;
    await expect(dialog).toHaveCount(0);
    expect(await page.locator(".publish-banner").allTextContents()).toEqual(bannerBefore);
    expect(readdirSync(out), "nothing is published after Cancel").toEqual([]);

    // Escape declines too.
    const again = startPublish(page, out);
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await again;
    await expect(dialog).toHaveCount(0);
    expect(readdirSync(out)).toEqual([]);
  } finally {
    removeTempVault(vault);
    removeTempVault(out);
  }
});
