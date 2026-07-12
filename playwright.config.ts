import { defineConfig } from "@playwright/test";

/**
 * Textree E2E — 실행 중인 Tauri 앱의 WebView2에 CDP로 연결해 검증한다.
 * mock IPC가 아니라 실제 파일시스템 백엔드를 거치므로 "파일시스템이 진실의
 * 원천"이라는 핵심 가치를 그대로 검증한다.
 *
 * Precondition: the app must be running with the remote debugging port open.
 *   npm run dev:e2e
 * (= tauri dev --config src-tauri/tauri.e2e.conf.json — WebView2 150+ ignores the
 * WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS env var, so the port is injected via the
 * window's additionalBrowserArgs instead. CDP is Windows/WebView2 only.)
 */
export default defineConfig({
  testDir: "./e2e",
  // 단일 webview에 CDP로 붙으므로 병렬 불가.
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  timeout: 30_000,
  expect: { timeout: 10_000 },
});
