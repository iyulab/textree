import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp } from "./helpers";

/**
 * D1 — 디자인 토큰 + 테마. 테마 토글이 <html data-theme>를 전환하고 실제 배경색이
 * 바뀌는지(토큰 적용) 실제 webview 에서 단언한다. 볼트 없이 동작(토글은 항상 표시).
 *
 * 시작 테마는 환경(OS prefers-color-scheme + 영속된 선택)에 따라 달라지므로,
 * 절대값이 아니라 **상대 전환**으로 검증한다(실사용 흐름과 동일).
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

test("테마 토글: data-theme 전환 + 배경색 실제 변화", async () => {
  const html = page.locator("html");
  const bg = () =>
    page.evaluate(() => getComputedStyle(document.body).backgroundColor);

  const before = (await html.getAttribute("data-theme")) ?? "light";
  const beforeBg = await bg();

  await page.getByRole("button", { name: "테마 전환" }).click();
  await expect(html).toHaveAttribute("data-theme", opposite(before));

  // 토큰이 실제로 적용됐다면 전환 후 배경색은 달라야 한다(전환 애니메이션 settle 대기).
  await expect.poll(bg).not.toBe(beforeBg);

  // 다시 토글하면 원래 테마로 복귀(+ 배경색 복귀).
  await page.getByRole("button", { name: "테마 전환" }).click();
  await expect(html).toHaveAttribute("data-theme", before);
  await expect.poll(bg).toBe(beforeBg);
});

test("테마 선택은 localStorage 에 영속", async () => {
  await page.getByRole("button", { name: "테마 전환" }).click();
  const applied = await page.locator("html").getAttribute("data-theme");
  const stored = await page.evaluate(() => localStorage.getItem("textree-theme"));
  // 토글은 명시 선택(light/dark)을 영속하며, 적용된 테마와 일치해야 한다.
  expect(stored).toBe(applied);
});
