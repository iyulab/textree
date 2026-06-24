import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, sampleVaultPath } from "./helpers";

/**
 * Settings overlay — host-absent E2E (CDP attach to WebView2).
 *
 * Host-absent: no TEXTREE_HOST_EXE / no real model running. Covers:
 *   • open / close (palette command mode + Ctrl+, accelerator)
 *   • three-section structure assertion
 *   • generation-toggle gating by embeddings consent (consent-only, host-independent)
 *   • theme segmented control → <html data-theme> attribute
 *
 * HOST-PRESENT MANUAL SMOKE (human gate — do not automate):
 * ──────────────────────────────────────────────────────────────────────────────
 * Prerequisites: app running with real host sidecar available
 *   ($env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev)
 *
 * 1. Open Settings (Ctrl+,). Enable "Embeddings & search" → badge shows "preparing" then "ready".
 * 2. Verify CPU/RAM usage indicates the host process is running.
 * 3. Disable "Embeddings & search" → host process should terminate; CPU/RAM released.
 * 4. Re-enable "Embeddings & search" → host spawns again; badge returns "ready".
 * 5. Enable "Q&A & chat" → confirm chat is available in the Chat panel.
 * 6. Disable "Q&A & chat" only → embedding host still running; chat unavailable.
 * (These steps require observing OS process monitor and real model inference — not automatable.)
 * ──────────────────────────────────────────────────────────────────────────────
 */

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
  // Load sample-vault so the app is in a defined state (Settings is available without a vault
  // but a vault ensures stable sidebar state across tests).
  await loadVault(page, sampleVaultPath());
});

test.afterAll(async () => {
  await browser?.close();
});

// ── helper: open Settings via palette command mode ──────────────────────────
async function openSettingsViaPalette(p: Page): Promise<void> {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-input")).toBeVisible();
  // ">settings" fuzzy-matches the "Settings" command (command mode requires ">" prefix).
  await p.getByTestId("palette-input").fill(">settings");
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  // Palette closes before the dialog appears.
  await expect(p.getByTestId("palette-overlay")).toHaveCount(0);
}

// ── Test 1: palette open → 3 sections → Escape closes ──────────────────────
test("opens Settings from the command palette and closes with Escape", async () => {
  await openSettingsViaPalette(page);

  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // Three content sections must be present (aria-label on <section> → role=region).
  await expect(dialog.getByRole("region", { name: "Appearance" })).toBeVisible();
  await expect(dialog.getByRole("region", { name: "Vault" })).toBeVisible();
  await expect(dialog.getByRole("region", { name: "Local AI" })).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

// ── Test 2: Ctrl+, accelerator opens Settings ───────────────────────────────
test("opens Settings with the Ctrl+, accelerator", async () => {
  await page.keyboard.press("Control+Comma");

  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

// ── Test 3: generation toggle gating by embeddings consent ──────────────────
// `generationDisabled` is derived from `!aiConsent` only (settings.helpers.ts:35) —
// no host-status dependency, so this test is valid host-absent.
//
// Consent flags persist in localStorage across serial tests (workers=1).
// Strategy: establish a known OFF state for embeddings before asserting, so the
// test is idempotent regardless of prior test order or localStorage contents.
test("generation toggle is disabled until embeddings consent is on", async () => {
  await page.keyboard.press("Control+Comma");
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // Checkbox accessible names come from the wrapping <label> text; &amp; → & in DOM.
  const embedding = dialog.getByRole("checkbox", { name: "Embeddings & search" });
  const generation = dialog.getByRole("checkbox", { name: "Q&A & chat" });

  // Establish a known OFF state: uncheck embeddings if it is currently on.
  // planEmbeddingToggle(false, ...) also cascades genConsent to false.
  if (await embedding.isChecked()) {
    await embedding.uncheck();
    // Wait for Svelte reactivity to propagate the disabled state.
    await expect(generation).toBeDisabled();
  }

  // Precondition: with embeddings off, generation must be disabled.
  await expect(generation).toBeDisabled();

  // Enable embeddings → generation becomes enabled (generationDisabled = !aiConsent).
  await embedding.check();
  await expect(generation).toBeEnabled();

  // Clean up: turn embeddings back off so we don't leave the host spawning in background.
  // (host-absent: the IPC call will fail silently per Settings.svelte catch block — safe.)
  await embedding.uncheck();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

// ── Test 4: theme segmented control switches <html data-theme> ───────────────
// themeButtons labels are "Auto" / "Light" / "Dark" (settings.helpers.ts:46).
// Role=radio comes from explicit role="radio" on the <button> elements in the radiogroup.
test("theme segmented control switches the applied theme", async () => {
  await page.keyboard.press("Control+Comma");
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  const html = page.locator("html");

  // Click "Dark" → <html data-theme="dark">
  await dialog.getByRole("radio", { name: "Dark" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");

  // Click "Light" → <html data-theme="light">
  await dialog.getByRole("radio", { name: "Light" }).click();
  await expect(html).toHaveAttribute("data-theme", "light");

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});
