import { chromium, type Browser, type Page, type Locator } from "@playwright/test";
import { resolve, join } from "node:path";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, readdirSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";

/**
 * Handshake file scripts/dev-e2e.mjs writes with the CDP endpoint it launched the app on.
 * Keep in sync with HANDSHAKE_FILE there. The launcher owns the port (it may fall back from an
 * occupied 9222 to a free one), so the specs read its decision rather than assuming a number.
 */
export const E2E_CDP_HANDSHAKE = join(tmpdir(), "textree-e2e-cdp.json");

function cdpEndpoint(): string {
  if (process.env.TEXTREE_E2E_CDP) return process.env.TEXTREE_E2E_CDP;
  if (existsSync(E2E_CDP_HANDSHAKE)) {
    const { endpoint } = JSON.parse(readFileSync(E2E_CDP_HANDSHAKE, "utf8")) as { endpoint?: string };
    if (endpoint) return endpoint;
  }
  return "http://localhost:9222";
}

const CDP_ENDPOINT = cdpEndpoint();
const APP_URL_FRAGMENT = "localhost:1420";

/**
 * Base directory the launcher forces the app to resolve its default vault under.
 * Keep in sync with DEFAULT_VAULT_BASE in scripts/dev-e2e.mjs. Specs that exercise the default-vault
 * flow assert the opened vault sits under this path, so they fail by name when the app was started
 * some other way instead of silently asserting against the developer's real notes.
 */
export const E2E_DEFAULT_VAULT_BASE = join(tmpdir(), "textree-e2e-default-vault");

/**
 * Base directory the launcher forces per-folder settings under.
 * Keep in sync with PERSONAL_BASE in scripts/dev-e2e.mjs.
 */
export const E2E_PERSONAL_BASE = join(tmpdir(), "textree-e2e-personal");

/**
 * Directory holding the settings the app keeps for one vault.
 *
 * Settings live outside the notes folder, keyed per folder, so a spec cannot read them at a path
 * it composes from the vault path alone. The key is the folder's own name followed by a digest of
 * its absolute path, so this looks the folder up by that name rather than recomputing the digest:
 * a copy of the digest in the specs would keep passing after the real one changed, which is worse
 * than not checking at all.
 *
 * Throws rather than returning null when the lookup is not unambiguous, so a spec says which of
 * the three it hit — never started through the launcher, the app wrote nothing, or two vaults in
 * the run share a folder name.
 */
export function sidecarDir(vaultPath: string): string {
  const vaults = join(E2E_PERSONAL_BASE, ".textree", "vaults");
  if (!existsSync(vaults)) {
    throw new Error(
      `No settings directory at ${vaults}. Start the app with \`npm run dev:e2e\`; ` +
        "scripts/dev-e2e.mjs points the app at this base.",
    );
  }
  const name = vaultPath.replace(/[/\\]+$/, "").split(/[/\\]/).pop() ?? "";
  const matches = readdirSync(vaults).filter((entry) => entry.startsWith(`${name}-`));
  if (matches.length !== 1) {
    throw new Error(
      `Expected exactly one settings folder for "${name}" under ${vaults}, found ${matches.length}` +
        (matches.length > 1 ? ` (${matches.join(", ")})` : ""),
    );
  }
  return join(vaults, matches[0]);
}

/**
 * Read one of the vault's settings files, or null when the app has not written it yet.
 * Callers poll this: the write is a side effect of a UI action, not something to await directly.
 */
export function readSidecar(vaultPath: string, name: string): unknown {
  try {
    return JSON.parse(readFileSync(join(sidecarDir(vaultPath), name), "utf8"));
  } catch {
    return null;
  }
}

/**
 * Absolute path to sample-vault (slash-normalized — the Tauri backend accepts both separators).
 * The launcher seeds it; failing here names that, instead of letting every dependent spec fail on
 * a missing tree item.
 */
export function sampleVaultPath(): string {
  const path = resolve(process.cwd(), "sample-vault");
  if (!existsSync(path)) {
    throw new Error(
      `sample-vault is missing at ${path}. Start the app with \`npm run dev:e2e\`; ` +
        "scripts/dev-e2e.mjs seeds it.",
    );
  }
  return path.replace(/\\/g, "/");
}

/**
 * Warnings and errors the app reported while the suite was driving it, oldest first.
 *
 * Some failures only warn: a settings write that does not land keeps the new state on screen while
 * disk keeps the old one, and the app carries on. A spec that later reads that file reports a value
 * mismatch with nothing to explain it, which is indistinguishable from a spec that is simply wrong.
 * Collecting these — and echoing them to the runner as they happen — is what makes the difference
 * visible on the run that hits it, rather than only on a run someone manages to reproduce.
 */
export const appLog: string[] = [];

/** Take everything collected so far and reset, so one test's noise is not read as another's. */
export function drainAppLog(): string[] {
  return appLog.splice(0, appLog.length);
}

function recordAppLog(page: Page): void {
  page.on("console", (msg) => {
    const type = msg.type();
    if (type !== "warning" && type !== "error") return;
    const line = `[app ${type}] ${msg.text()}`;
    appLog.push(line);
    console.log(line);
  });
  page.on("pageerror", (err) => {
    const line = `[app pageerror] ${err.message}`;
    appLog.push(line);
    console.log(line);
  });
}

/**
 * Connect to the running Textree WebView2 via CDP and return the app page.
 * When done, the caller closes only the CDP connection with browser.close() (the real app stays running).
 */
export async function connectToApp(): Promise<{ browser: Browser; page: Page }> {
  const browser = await chromium.connectOverCDP(CDP_ENDPOINT);
  for (const ctx of browser.contexts()) {
    for (const p of ctx.pages()) {
      if (p.url().includes(APP_URL_FRAGMENT)) {
        recordAppLog(p);
        // Wait until the dev test bridge is up (guarantees onMount completed).
        await p.waitForFunction(
          () => Boolean((window as unknown as { __textreeTest?: unknown }).__textreeTest),
          { timeout: 10_000 },
        );
        return { browser, page: p };
      }
    }
  }
  await browser.close();
  throw new Error(
    `Could not find the Textree app page via CDP (${CDP_ENDPOINT}). ` +
      `Make sure the app is running with 'npm run dev:e2e' — it picks the CDP port and records it in ` +
      `${E2E_CDP_HANDSHAKE}, which this reads unless TEXTREE_E2E_CDP overrides the endpoint.`,
  );
}

/** Open a vault via the dev bridge, bypassing the dialog. */
export async function loadVault(page: Page, vaultPath: string): Promise<void> {
  await page.evaluate(
    (v) =>
      (window as unknown as { __textreeTest: { loadVault: (p: string) => Promise<void> } }).__textreeTest.loadVault(v),
    vaultPath,
  );
}

/**
 * Create an isolated temporary vault. files = { relativePath: content }.
 * Isolated per test so edit/create/delete and sync tests don't pollute sample-vault.
 * The returned path is slash-normalized (can be passed straight to the app dev bridge).
 */
export function createTempVault(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "textree-e2e-"));
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(resolve(abs, ".."), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  return dir.replace(/\\/g, "/");
}

export function removeTempVault(vaultPath: string): void {
  // The app still has this vault open when a spec tears it down, and on Windows a write it lands
  // mid-removal (a version, the watcher's canary) makes the directory ENOTEMPTY/EBUSY. rmSync's
  // own retries are the documented remedy for exactly those codes.
  rmSync(vaultPath, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}

/** Read a file inside the vault directly from disk (to verify what the app wrote). */
export function readVaultFile(vaultPath: string, rel: string): string {
  return readFileSync(join(vaultPath, rel), "utf8");
}

/** List directory entries inside the vault (empty array if none). For verifying attachment saves. */
export function listVaultDir(vaultPath: string, rel: string): string[] {
  try {
    return readdirSync(join(vaultPath, rel));
  } catch {
    return [];
  }
}

/** Write a file inside the vault directly to disk (simulating an external change — for watcher verification). */
export function writeVaultFile(vaultPath: string, rel: string, content: string): void {
  const abs = join(vaultPath, rel);
  mkdirSync(resolve(abs, ".."), { recursive: true });
  writeFileSync(abs, content, "utf8");
}

// Tree DnD transfer MIME (mirrors the single source of truth DRAG_MIME in TreeView.svelte).
const DRAG_MIME = "application/x-textree-path";

/**
 * HTML5 native DnD simulation. Playwright's default drag is mouse-based and
 * doesn't populate dataTransfer, so we dispatch dragstart→dragover→drop→dragend
 * directly with the same DataTransfer instance (the app's setData/getData contract).
 */
export async function dragNodeOnto(page: Page, src: Locator, dst: Locator): Promise<void> {
  const srcEl = await src.elementHandle();
  const dstEl = await dst.elementHandle();
  if (!srcEl || !dstEl) throw new Error("Could not find the drag source/target element");
  await page.evaluate(
    ([s, d]) => {
      const dt = new DataTransfer();
      const fire = (el: Element, type: string) =>
        el.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
      fire(s, "dragstart");
      fire(d, "dragover");
      fire(d, "drop");
      fire(s, "dragend");
    },
    [srcEl, dstEl] as const,
  );
}

export { DRAG_MIME };
