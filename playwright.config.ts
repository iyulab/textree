import { defineConfig } from "@playwright/test";

/**
 * Textree E2E — connects over CDP to the WebView2 of the running Tauri app.
 * Nothing is mocked: every assertion goes through the real filesystem backend, so the suite
 * verifies the property the app is built on, that the files on disk are the source of truth.
 *
 * Precondition: the app must be running with the remote debugging port open.
 *   npm run dev:e2e
 * (= tauri dev --config src-tauri/tauri.e2e.conf.json — WebView2 150+ ignores the
 * WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS env var, so the port is injected via the
 * window's additionalBrowserArgs instead. CDP is Windows/WebView2 only.)
 */
export default defineConfig({
  testDir: "./e2e",
  // A single webview is shared over one CDP connection, so the suite cannot run in parallel.
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  timeout: 30_000,
  expect: { timeout: 10_000 },
});
