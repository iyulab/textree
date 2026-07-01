import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * D1 — design tokens + theme. Assert in the real webview that the theme toggle
 * switches <html data-theme> and the actual background color changes (tokens
 * applied). Works without a vault (the toggle is always shown).
 *
 * The starting theme varies with the environment (OS prefers-color-scheme +
 * persisted selection), so verify by **relative switch** rather than absolute
 * value (same as the real usage flow).
 */

let browser: Browser;
let page: Page;

const opposite = (t: string) => (t === "dark" ? "light" : "dark");

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

  await page.getByRole("button", { name: "Toggle theme" }).click();
  await expect(html).toHaveAttribute("data-theme", opposite(before));

  // If tokens are actually applied, the background color must differ after the switch (wait for the transition animation to settle).
  await expect.poll(bg).not.toBe(beforeBg);
  await expect.poll(scheme).toBe(opposite(before));

  // Toggling again returns to the original theme (+ background color reverts).
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await expect(html).toHaveAttribute("data-theme", before);
  await expect.poll(bg).toBe(beforeBg);
  await expect.poll(scheme).toBe(before);
});

test("theme selection persists in localStorage", async () => {
  await page.getByRole("button", { name: "Toggle theme" }).click();
  const applied = await page.locator("html").getAttribute("data-theme");
  const stored = await page.evaluate(() => localStorage.getItem("textree-theme"));
  // The toggle persists the explicit selection (light/dark) and must match the applied theme.
  expect(stored).toBe(applied);
});
