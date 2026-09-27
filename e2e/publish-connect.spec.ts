/**
 * E2E — in-app sign-in for web publishing (browser OAuth loopback + PKCE).
 *
 * Gated profile (cannot share an app instance with the normal suite, because the app reads its
 * backend base URLs from the environment at launch):
 *
 *   Terminal 1:  $env:TEXTREE_APP_BASE = "http://127.0.0.1:8787"
 *                $env:TEXTREE_API_BASE = "http://127.0.0.1:8788"
 *                npm run dev:e2e
 *   Terminal 2:  $env:TEXTREE_CONNECT_E2E = "mock"
 *                npm run test:e2e -- publish-connect
 *
 * Without TEXTREE_CONNECT_E2E the whole file skips, so the default suite never triggers a sign-in
 * against the real service (which would open a browser and block for three minutes).
 *
 * What this covers that unit tests cannot: the button in the app actually starts a loopback
 * listener, hands the cloud a fresh state + PKCE challenge, accepts only the callback carrying its
 * own state, exchanges the code, stores the granted token, and renders a failure where a user would
 * see it.
 *
 * Completing a sign-in is safe because the app under test is a development build, and those are
 * compiled to use their own credential entry (`secret_store.rs`) — the entry the installed app uses
 * is unreachable from here. The success test proves that separation rather than trusting it: it
 * reads both entries and fails if the installed app's one moved.
 *
 * Selectors come from Settings.svelte (Advanced section) and +page.svelte (publish banner).
 */
import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, createTempVault, loadVault, removeTempVault } from "./helpers";
import { deliverCallback, startCloudMock, type CloudMock } from "./cloud-mock";
import {
  DEV_PUBLISH_TARGET,
  PRODUCTION_PUBLISH_TARGET,
  readCredential,
  sameRecord,
} from "./credential-probe";

const PROFILE = process.env.TEXTREE_CONNECT_E2E;

let browser: Browser;
let page: Page;
let mock: CloudMock;

test.beforeAll(async () => {
  if (PROFILE !== "mock") return;
  mock = await startCloudMock();
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
  await mock?.close();
});

async function hasPublishToken(p: Page): Promise<boolean> {
  return p.evaluate(() =>
    (window as unknown as { __textreeTest: { hasPublishToken: () => Promise<boolean> } }).__textreeTest.hasPublishToken(),
  );
}

async function openPublishSettings(p: Page) {
  await p.keyboard.press("Control+p");
  await expect(p.getByTestId("palette-input")).toBeVisible();
  await p.getByTestId("palette-input").fill(">settings");
  await expect(p.getByTestId("palette-item").first()).toBeVisible();
  await p.keyboard.press("Enter");
  const dialog = p.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // The publishing controls live in a <details>. Expand it only when it is closed — clicking a
  // summary unconditionally toggles, so a blind click collapses a section a previous test left open.
  const advanced = dialog.locator("details.byo-advanced");
  if (!(await advanced.evaluate((el: HTMLDetailsElement) => el.open))) {
    await advanced.locator("summary").click();
  }
  await expect(advanced).toHaveAttribute("open", "");
  // The publishing controls, not the whole dialog: the Backup section has a Connect and a
  // Disconnect of its own.
  return advanced;
}

/** Closes Settings and waits for it to go away, so the next test starts from a known state. */
async function closeSettings(p: Page) {
  const dialog = p.getByRole("dialog", { name: "Settings" });
  for (let i = 0; i < 3 && (await dialog.isVisible()); i++) {
    await dialog.click({ position: { x: 5, y: 5 } }); // focus the dialog so it receives the key
    await p.keyboard.press("Escape");
    await p.waitForTimeout(200);
  }
  await expect(dialog).toBeHidden();
}

/** base64url, no padding — the shape both the state and the S256 challenge must have. */
const BASE64URL_32_BYTES = /^[A-Za-z0-9_-]{43}$/;

/** Returns the app to the not-yet-connected state, whatever a previous test left behind. */
async function disconnect(p: Page) {
  if (!(await hasPublishToken(p))) return;
  const dialog = await openPublishSettings(p);
  await dialog.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect.poll(() => hasPublishToken(p)).toBe(false);
  await closeSettings(p);
}

test.describe("in-app sign-in", () => {
  test.beforeEach(async () => {
    test.skip(PROFILE !== "mock", "Set TEXTREE_CONNECT_E2E=mock and launch the app against the mock backend.");
    // Every test here starts from "nothing stored yet". A development build keeps its token in its
    // own entry, so clearing it costs nothing and no longer has to be worked around by skipping.
    await disconnect(page);
    mock.reset();
  });

  test("hands the cloud a fresh loopback port and PKCE challenge, then surfaces the failure", async () => {
    const dialog = await openPublishSettings(page);
    const connect = dialog.getByRole("button", { name: "Connect", exact: true });
    await expect(connect).toBeVisible();

    // Not awaited: the click resolves only when the whole sign-in settles.
    void connect.click();

    const auth = await mock.waitForAuthorize();
    expect(auth.port, "a loopback port was allocated").toMatch(/^\d{4,5}$/);
    expect(auth.state, "state is 32 bytes of base64url").toMatch(BASE64URL_32_BYTES);
    expect(auth.codeChallenge, "challenge is a base64url SHA-256").toMatch(BASE64URL_32_BYTES);
    expect(auth.codeChallengeMethod).toBe("S256");
    expect(auth.state).not.toBe(auth.codeChallenge);

    // Play the browser's part.
    expect(await deliverCallback(auth.port, "e2e-code", auth.state)).toBe(200);

    // The desktop exchanges the code, sending the verifier — never the challenge.
    await expect
      .poll(() => mock.exchangeBodies().length, { message: "the desktop exchanged the code" })
      .toBe(1);
    const body = mock.exchangeBodies()[0] as { code?: string; codeVerifier?: string };
    expect(body.code).toBe("e2e-code");
    expect(body.codeVerifier, "verifier is 32 bytes of base64url").toMatch(BASE64URL_32_BYTES);
    expect(body.codeVerifier).not.toBe(auth.codeChallenge);

    // The mock rejects, and the user sees why.
    await expect(dialog.getByText(/Failed:.*sign-in expired or was invalid/i)).toBeVisible();

    expect(await hasPublishToken(page), "a failed sign-in must not store anything").toBe(false);
    await closeSettings(page);
  });

  test("ignores a callback carrying someone else's state and keeps waiting for its own", async () => {
    const dialog = await openPublishSettings(page);
    void dialog.getByRole("button", { name: "Connect", exact: true }).click();

    const auth = await mock.waitForAuthorize();

    // A forged or stray callback must not release the code.
    expect(await deliverCallback(auth.port, "forged-code", "not-our-state")).toBe(404);
    expect(mock.exchangeBodies(), "nothing was exchanged for the forged callback").toHaveLength(0);

    // The listener is still up and still bound to its own state.
    expect(await deliverCallback(auth.port, "e2e-code", auth.state)).toBe(200);
    await expect.poll(() => mock.exchangeBodies().length).toBe(1);
    expect((mock.exchangeBodies()[0] as { code?: string }).code).toBe("e2e-code");

    expect(await hasPublishToken(page)).toBe(false);
    await closeSettings(page);
  });

  test("a completed sign-in stores the token, and stores it away from the installed app", async () => {
    // Read both entries first: the one this build is expected to write, and the one it must never
    // touch. The second reading is what turns "we namespaced it" into something the suite checks.
    const productionBefore = readCredential(PRODUCTION_PUBLISH_TARGET);
    expect(readCredential(DEV_PUBLISH_TARGET).present, "starts disconnected").toBe(false);

    mock.grantToken("tk_e2e_granted");

    const dialog = await openPublishSettings(page);
    void dialog.getByRole("button", { name: "Connect", exact: true }).click();

    const auth = await mock.waitForAuthorize();
    expect(await deliverCallback(auth.port, "e2e-code", auth.state)).toBe(200);

    // The granted token reaches the keychain, and the UI switches to connected.
    await expect.poll(() => hasPublishToken(page), { message: "the token was stored" }).toBe(true);
    await expect(dialog.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
    await expect(dialog.getByText(/Connected to web publishing/i)).toBeVisible();

    // It landed in this build's own entry...
    expect(readCredential(DEV_PUBLISH_TARGET).present, "the development entry now holds it").toBe(true);
    // ...and the entry the installed app reads is byte-for-byte where it was.
    expect(
      sameRecord(readCredential(PRODUCTION_PUBLISH_TARGET), productionBefore),
      `a test wrote to ${PRODUCTION_PUBLISH_TARGET} — the credential namespacing in secret_store.rs is gone, ` +
        "and a real publishing token has just been overwritten",
    ).toBe(true);

    // Disconnecting takes it back out, so the next test starts clean.
    await dialog.getByRole("button", { name: "Disconnect", exact: true }).click();
    await expect.poll(() => hasPublishToken(page)).toBe(false);
    expect(readCredential(DEV_PUBLISH_TARGET).present).toBe(false);
    await closeSettings(page);
  });

  test("publishing without a token starts the sign-in instead of dead-ending", async () => {
    const vault = createTempVault({ "note.md": "# Publish source\n" });
    try {
      await loadVault(page, vault);

      await page.keyboard.press("Control+p");
      await expect(page.getByTestId("palette-input")).toBeVisible();
      await page.getByTestId("palette-input").fill(">publish to web");
      await expect(page.getByTestId("palette-item").first()).toBeVisible();
      await page.keyboard.press("Enter");

      // What goes out is confirmed before anything else happens, the sign-in included.
      await expect(page.getByTestId("publish-confirm-go")).toBeEnabled({ timeout: 10000 });
      await page.getByTestId("publish-confirm-go").click();

      // No token is stored, so the publish path must run the sign-in first rather than telling the
      // user to go and paste one.
      const auth = await mock.waitForAuthorize();
      expect(auth.codeChallengeMethod).toBe("S256");

      expect(await deliverCallback(auth.port, "e2e-code", auth.state)).toBe(200);
      await expect.poll(() => mock.exchangeBodies().length).toBe(1);

      // The failure surfaces on the publish banner, with a retry offered.
      const banner = page.locator(".publish-banner.error");
      await expect(banner).toBeVisible();
      await expect(banner.getByRole("button", { name: /retry/i })).toBeVisible();

      expect(await hasPublishToken(page)).toBe(false);
      await banner.getByRole("button", { name: /dismiss/i }).click();
    } finally {
      removeTempVault(vault);
    }
  });
});
