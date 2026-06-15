import { chromium, type Browser, type Page, type Locator } from "@playwright/test";
import { resolve, join } from "node:path";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";

const CDP_ENDPOINT = "http://localhost:9222";
const APP_URL_FRAGMENT = "localhost:1420";

/** Absolute path to sample-vault (slash-normalized — the Tauri backend accepts both separators). */
export function sampleVaultPath(): string {
  return resolve(process.cwd(), "sample-vault").replace(/\\/g, "/");
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
    `Textree 앱 페이지를 CDP에서 찾지 못했습니다(${CDP_ENDPOINT}). ` +
      `'$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev'로 앱을 띄웠는지 확인하세요.`,
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
  rmSync(vaultPath, { recursive: true, force: true });
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
  if (!srcEl || !dstEl) throw new Error("드래그 소스/대상 엘리먼트를 찾지 못했습니다");
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
