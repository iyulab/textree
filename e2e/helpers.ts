import { chromium, type Browser, type Page, type Locator } from "@playwright/test";
import { resolve, join } from "node:path";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";

const CDP_ENDPOINT = "http://localhost:9222";
const APP_URL_FRAGMENT = "localhost:1420";

/** sample-vault 절대경로(슬래시 정규화 — Tauri 백엔드가 양쪽 구분자 모두 수용). */
export function sampleVaultPath(): string {
  return resolve(process.cwd(), "sample-vault").replace(/\\/g, "/");
}

/**
 * 실행 중인 Textree WebView2에 CDP로 연결해 앱 페이지를 반환한다.
 * 호출자는 끝나고 browser.close()로 CDP 연결만 끊는다(실제 앱은 유지됨).
 */
export async function connectToApp(): Promise<{ browser: Browser; page: Page }> {
  const browser = await chromium.connectOverCDP(CDP_ENDPOINT);
  for (const ctx of browser.contexts()) {
    for (const p of ctx.pages()) {
      if (p.url().includes(APP_URL_FRAGMENT)) {
        // dev 테스트 브리지가 올라올 때까지 대기(onMount 완료 보장).
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

/** 다이얼로그를 우회해 dev 브리지로 볼트를 연다. */
export async function loadVault(page: Page, vaultPath: string): Promise<void> {
  await page.evaluate(
    (v) =>
      (window as unknown as { __textreeTest: { loadVault: (p: string) => Promise<void> } }).__textreeTest.loadVault(v),
    vaultPath,
  );
}

/**
 * 격리된 임시 볼트를 만든다. files = { 상대경로: 내용 }.
 * 편집/생성/삭제·동기화 테스트가 sample-vault를 오염시키지 않도록 매 테스트 격리.
 * 반환 경로는 슬래시 정규화(앱 dev 브리지에 그대로 전달 가능).
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

/** 볼트 내 파일을 디스크에서 직접 읽는다(앱이 기록한 결과 검증용). */
export function readVaultFile(vaultPath: string, rel: string): string {
  return readFileSync(join(vaultPath, rel), "utf8");
}

/** 볼트 내 디렉터리 항목 목록(없으면 빈 배열). 첨부 저장 검증용. */
export function listVaultDir(vaultPath: string, rel: string): string[] {
  try {
    return readdirSync(join(vaultPath, rel));
  } catch {
    return [];
  }
}

/** 볼트 내 파일을 디스크에 직접 쓴다(외부 변경 시뮬레이션 — 워처 검증용). */
export function writeVaultFile(vaultPath: string, rel: string, content: string): void {
  const abs = join(vaultPath, rel);
  mkdirSync(resolve(abs, ".."), { recursive: true });
  writeFileSync(abs, content, "utf8");
}

// 트리 DnD 전송 MIME(TreeView.svelte의 DRAG_MIME 단일 출처를 미러링).
const DRAG_MIME = "application/x-textree-path";

/**
 * HTML5 네이티브 DnD 시뮬레이션. Playwright 기본 드래그는 마우스 기반이라
 * dataTransfer를 채우지 못하므로, 동일한 DataTransfer 인스턴스로
 * dragstart→dragover→drop→dragend를 직접 디스패치한다(앱의 setData/getData 계약).
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
