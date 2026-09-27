import { test, expect, type Browser, type Page } from "@playwright/test";
import { mkdtempSync, existsSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  connectToApp,
  expectOpenNote,
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

    // The injected tokens were rewritten for the OS preference (auto theme on the static site):
    // the dark block now lives inside a prefers-color-scheme media query wrapping :root.
    const tokens = readFileSync(join(out, "tokens.css"), "utf8");
    const mediaAt = tokens.indexOf("@media (prefers-color-scheme: dark)");
    expect(mediaAt).toBeGreaterThanOrEqual(0);
    expect(tokens.slice(mediaAt)).toContain(":root {");

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
    // Give one note a version, so only the other goes out with changes that have none.
    await page.getByRole("treeitem", { name: /kept/ }).click();
    await expectOpenNote(page, "kept");
    await page.keyboard.press("Control+Shift+S");
    await expect(page.getByTestId("add-version")).toBeVisible();
    await page.getByTestId("add-version-confirm").click();
    await expect(page.getByTestId("add-version")).toHaveCount(0);

    // Whatever an earlier publish left on the banner stays as it was: declining changes nothing.
    const bannerBefore = await page.locator(".publish-banner").allTextContents();
    const publishing = startPublish(page, out);
    const dialog = page.getByRole("dialog", { name: "Publish this folder?" });
    await expect(dialog).toBeVisible();
    await expect(page.getByTestId("publish-confirm-summary")).toHaveText(
      `2 notes and 1 other file in this folder will be published into ${out}.`,
    );
    const unrecorded = page.getByTestId("publish-confirm-unrecorded");
    await expect(unrecorded).toContainText("1 note has changes you haven't added as a version");
    await expect(unrecorded).toContainText("draft");
    await expect(unrecorded).not.toContainText("kept");
    await expect(page.getByTestId("publish-confirm-hidden")).toHaveText(
      "Hidden files are left out: .env",
    );
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

test("a hidden file in the vault stays out of the published site", async () => {
  const vault = createTempVault({
    "note.md": "# Note\n",
    ".env": "SECRET=1\n",
    "sub/page.md": "# Page\n",
    "sub/.gitignore": "*.tmp\n",
  });
  const out = mkdtempSync(join(tmpdir(), "textree-pub-")).replace(/\\/g, "/");
  try {
    await loadVault(page, vault);
    await publishTo(page, out);
    await expect(page.locator(".publish-banner.ok")).toContainText("Published");
    expect(existsSync(join(out, "note.html"))).toBe(true);
    expect(existsSync(join(out, "sub", "page.html"))).toBe(true);
    expect(existsSync(join(out, ".env")), ".env must not be published").toBe(false);
    expect(existsSync(join(out, "sub", ".gitignore")), "nor a nested dot-file").toBe(false);
  } finally {
    removeTempVault(vault);
    removeTempVault(out);
  }
});
