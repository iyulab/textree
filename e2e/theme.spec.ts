import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * Design tokens + theme. Assert in the real webview that the theme toggle
 * switches <html data-theme> and the actual background color changes (tokens
 * applied). The toggle lives in the ⋮ app menu (the standalone sidebar button
 * was absorbed into it), and the ungated View category is always shown.
 *
 * The starting theme varies with the environment (OS prefers-color-scheme +
 * persisted selection), so verify by **relative switch** rather than absolute
 * value (same as the real usage flow).
 */

let browser: Browser;
let page: Page;

const opposite = (t: string) => (t === "dark" ? "light" : "dark");

/** Toggle the theme via the ⋮ app menu (same path a user takes). */
async function toggleTheme(p: Page): Promise<void> {
  await p.getByTestId("app-menu-button").click();
  await p.getByTestId("app-menu-item").filter({ hasText: "Toggle theme" }).click();
  await expect(p.getByTestId("app-menu")).toHaveCount(0);
}

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

test("theme toggle: data-theme switch + actual background color change", async () => {
  const html = page.locator("html");
  const bg = () =>
    page.evaluate(() => getComputedStyle(document.body).backgroundColor);

  // Native UI (scrollbars, form controls) must follow the theme via color-scheme — without it
  // the WebView paints a light scrollbar in the dark theme. Assert it tracks the same switch.
  const scheme = () =>
    page.evaluate(() => getComputedStyle(document.documentElement).colorScheme);

  const before = (await html.getAttribute("data-theme")) ?? "light";
  const beforeBg = await bg();
  expect(await scheme()).toBe(before);

  await toggleTheme(page);
  await expect(html).toHaveAttribute("data-theme", opposite(before));

  // If tokens are actually applied, the background color must differ after the switch (wait for the transition animation to settle).
  await expect.poll(bg).not.toBe(beforeBg);
  await expect.poll(scheme).toBe(opposite(before));

  // Toggling again returns to the original theme (+ background color reverts).
  await toggleTheme(page);
  await expect(html).toHaveAttribute("data-theme", before);
  await expect.poll(bg).toBe(beforeBg);
  await expect.poll(scheme).toBe(before);
});

test("theme selection persists in localStorage", async () => {
  await toggleTheme(page);
  const applied = await page.locator("html").getAttribute("data-theme");
  const stored = await page.evaluate(() => localStorage.getItem("textree-theme"));
  // The toggle persists the explicit selection (light/dark) and must match the applied theme.
  expect(stored).toBe(applied);
});
