import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, createTempVault, loadVault, removeTempVault, sampleVaultPath } from "./helpers";

/**
 * "Publish to web" (slice 2c — in-app auth) — host-absent-safe E2E (CDP attach to WebView2).
 *
 * With no token stored, invoking "Publish to web" now starts an in-app browser sign-in
 * (OAuth loopback + PKCE), which opens a real browser window and waits for a redirect — not
 * safe or completable in CI. So this spec only exercises the non-destructive surfaces:
 *   - the command surfaces in the palette (never presses Enter — executing it would upload if a
 *     token is stored, or open a browser sign-in if not),
 *   - Settings ▸ Advanced exposes the web-publishing connect/disconnect controls, branching
 *     read-only on the dev bridge's `hasPublishToken()`.
 *
 * The real sign-in + upload round-trip is owner e2e (slice 2c spec §6).
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

async function hasPublishTokenStored(p: Page): Promise<boolean> {
  return p.evaluate(() =>
    (
      window as unknown as { __textreeTest: { hasPublishToken: () => Promise<boolean> } }
    ).__textreeTest.hasPublishToken(),
  );
}

async function openSettingsViaPalette(p: Page): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-input")).toBeVisible();
  await p.getByTestId("palette-input").fill(">settings");
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

test("the command palette surfaces 'Publish to web'", async () => {
  await loadVault(page, sampleVaultPath());

  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();

  // Command mode ('>' prefix), fuzzy-matched by the full title.
  await page.getByTestId("palette-input").fill(">Publish to web");
  await expect(page.getByTestId("palette-item").filter({ hasText: "Publish to web" })).toBeVisible();

  // Do NOT press Enter — with a token it uploads; without one it opens a browser sign-in.
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});

test("'Publish to web' asks first, and Cancel neither signs in nor uploads", async () => {
  // Declining happens before the sign-in and the upload, so this is safe with or without a token.
  // A throwaway folder all the same: were the question ever skipped, this is what would go out.
  const vault = createTempVault({ "note.md": "# Note\n", "extra.txt": "x" });
  try {
    await loadVault(page, vault);
    await page.keyboard.press("Control+p");
    await page.getByTestId("palette-input").fill(">Publish to web");
    await expect(page.getByTestId("palette-item").first()).toContainText("Publish to web");
    await page.keyboard.press("Enter");

    const dialog = page.getByRole("dialog", { name: "Publish this folder?" });
    await expect(dialog).toBeVisible();
    await expect(page.getByTestId("publish-confirm-summary")).toHaveText(
      "1 note and 1 other file in this folder will be published to the web.",
    );
    await page.getByTestId("publish-confirm-cancel").click();
    await expect(dialog).toHaveCount(0);
    await expect(page.locator(".publish-banner.publishing")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});

test("Settings ▸ Advanced exposes the web-publishing connect controls", async () => {
  await openSettingsViaPalette(page);
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  await dialog.getByText("Advanced: custom AI server").click();

  // The intro copy is present regardless of connection state.
  await expect(dialog.getByText(/Connect your account to publish/i)).toBeVisible();

  // Branch (read-only, non-destructive) on whether a token already exists in the keychain:
  // the control toggles between "Connect" and a connected + "Disconnect" state.
  if (await hasPublishTokenStored(page)) {
    await expect(dialog.getByText(/Connected to web publishing/i)).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Disconnect" })).toBeVisible();
  } else {
    await expect(dialog.getByRole("button", { name: "Connect", exact: true })).toBeVisible();
  }

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});
