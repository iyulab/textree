import { defineConfig } from "@playwright/test";

/**
 * Textree E2E — 실행 중인 Tauri 앱의 WebView2에 CDP로 연결해 검증한다.
 * mock IPC가 아니라 실제 파일시스템 백엔드를 거치므로 "파일시스템이 진실의
 * 원천"이라는 핵심 가치를 그대로 검증한다.
 *
 * 사전 조건: 앱이 원격 디버깅 포트로 떠 있어야 한다.
 *   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
 *   npm run tauri dev
 * (CDP는 Windows/WebView2에서만 지원된다.)
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
