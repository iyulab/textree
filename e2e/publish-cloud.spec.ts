import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * "Publish to web" (slice 2b) — host-absent-safe E2E (CDP attach to WebView2).
 *
 * The publish token lives in the machine-global OS keychain (no test teardown — see
 * byo_secret.rs), and invoking "Publish to web" WITH a token stored triggers a real
 * render+upload to the owner's live pub.textree.me. So this spec:
 *   - always asserts the command surfaces in the palette (never presses Enter for that check),
 *   - always asserts Settings ▸ Advanced exposes the web publish token controls,
 *   - only invokes the command (safe: it guides to Settings and returns without uploading)
 *     when the dev bridge's non-destructive `hasPublishToken()` read confirms no token is
 *     stored; otherwise it skips that one assertion with a clear reason.
 *
 * The real upload round-trip is out of scope here (owner e2e, per the slice 2b spec).
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

  // Do NOT press Enter — executing it could trigger a real upload if a token is stored
  // in this machine's keychain. Close via Escape instead.
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
});

test("Settings ▸ Advanced exposes the web publish token controls", async () => {
  await openSettingsViaPalette(page);
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  await dialog.getByText("Advanced: custom AI server").click();

  // The intro copy is present regardless of whether a token is already stored.
  await expect(dialog.getByText(/Paste the token from app\.textree\.me/i)).toBeVisible();

  // Branch (read-only, non-destructive) on whether a token already exists in the keychain:
  // the field toggles between an entry form and a "stored" confirmation.
  if (await hasPublishTokenStored(page)) {
    await expect(dialog.getByText(/Publish token stored/i)).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Remove token" })).toBeVisible();
  } else {
    await expect(dialog.getByLabel("Web publish token")).toBeVisible();
    await expect(dialog.getByRole("button", { name: /Save token/i })).toBeVisible();
  }

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("Publish to web without a stored token guides to Settings instead of uploading", async () => {
  const hasToken = await hasPublishTokenStored(page);
  test.skip(
    hasToken,
    "A real publish token is stored in this machine's OS keychain — invoking 'Publish to web' " +
      "would trigger a real render+upload to the owner's live pub.textree.me. This guide-notice " +
      "path is only safe to exercise when no token is stored (e.g. a fresh machine).",
  );

  await loadVault(page, sampleVaultPath());

  await page.keyboard.press("Control+p");
  await expect(page.getByTestId("palette-overlay")).toBeVisible();
  await page.getByTestId("palette-input").fill(">Publish to web");
  await expect(page.getByTestId("palette-item").filter({ hasText: "Publish to web" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

  // No upload attempted — the notice points the user at Settings instead.
  await expect(page.locator(".publish-banner.error")).toContainText(/token in Settings/i);
});
