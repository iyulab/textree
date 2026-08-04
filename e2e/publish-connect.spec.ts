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
 * against the real service (which would open a browser and block for three minutes). The tests
 * also skip when a publish token is already stored: they cover the not-yet-connected path, and
 * clearing a real token to reach that state would be destructive and unrecoverable here.
 *
 * What this covers that unit tests cannot: the button in the app actually starts a loopback
 * listener, hands the cloud a fresh state + PKCE challenge, accepts only the callback carrying its
 * own state, exchanges the code, and renders the failure where a user would see it.
 *
 * What it deliberately does NOT cover: a successful sign-in. The mock always rejects the exchange,
 * because succeeding would write to the machine-wide OS credential store and destroy whatever real
 * publishing token this machine holds. Every test asserts the store is still untouched afterwards.
 * The success path is covered by the Rust round-trip tests and by a manual owner sign-in.
 *
 * Selectors come from Settings.svelte (Advanced section) and +page.svelte (publish banner).
 */
import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, createTempVault, loadVault, removeTempVault } from "./helpers";
import { deliverCallback, startCloudMock, type CloudMock } from "./cloud-mock";

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
  return dialog;
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

test.describe("in-app sign-in", () => {
  test.beforeEach(async () => {
    test.skip(PROFILE !== "mock", "Set TEXTREE_CONNECT_E2E=mock and launch the app against the mock backend.");
    // These tests exercise the path taken when nothing is stored yet. On a machine that is already
    // connected the app correctly skips the sign-in, so there is nothing here to observe — and the
    // spec will not clear a real token to manufacture the precondition, because it cannot put it
    // back (a token can only be minted by a real sign-in). Disconnect deliberately to run these.
    test.skip(
      await hasPublishToken(page),
      "Requires a machine with no publish token stored — disconnect first (Settings ▸ Advanced ▸ Disconnect).",
    );
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

  test("publishing without a token starts the sign-in instead of dead-ending", async () => {
    const vault = createTempVault({ "note.md": "# Publish source\n" });
    try {
      await loadVault(page, vault);

      await page.keyboard.press("Control+p");
      await expect(page.getByTestId("palette-input")).toBeVisible();
      await page.getByTestId("palette-input").fill(">publish to web");
      await expect(page.getByTestId("palette-item").first()).toBeVisible();
      await page.keyboard.press("Enter");

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
