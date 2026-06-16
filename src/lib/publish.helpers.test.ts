import { expect, test } from "vitest";
import { toPublishTokens } from "./publish.helpers";

test("wraps the dark-theme block in a prefers-color-scheme media query on :root", () => {
  const css = ':root {\n  --c: white;\n}\n[data-theme="dark"] {\n  --c: black;\n}\n';
  const out = toPublishTokens(css);
  // Light :root is the default and untouched.
  expect(out).toContain(":root {\n  --c: white;\n}");
  // The dark vars now apply via the OS preference (no JS toggle exists on a static site).
  expect(out).toContain("@media (prefers-color-scheme: dark)");
  expect(out).toContain("--c: black;");
  // The attribute selector is gone — nothing sets data-theme on the published page.
  expect(out).not.toContain('[data-theme="dark"]');
});

test("leaves css without a dark-theme block unchanged", () => {
  const css = ":root {\n  --c: white;\n}\n";
  expect(toPublishTokens(css)).toBe(css);
});

test("preserves rules that follow the dark block (e.g. reduced-motion media)", () => {
  const css =
    ':root{--a:1}\n[data-theme="dark"]{--a:2}\n@media (prefers-reduced-motion: reduce){:root{--x:0}}\n';
  const out = toPublishTokens(css);
  expect(out).toContain("prefers-reduced-motion");
  expect(out.indexOf("prefers-color-scheme")).toBeLessThan(
    out.indexOf("prefers-reduced-motion"),
  );
});

test("keeps the light :root and the dark vars distinct after the rewrite", () => {
  const css = ':root{--bg:#fff;--fg:#111}\n[data-theme="dark"]{--bg:#111;--fg:#eee}\n';
  const out = toPublishTokens(css);
  expect(out).toContain("--bg:#fff");
  expect(out).toContain("--bg:#111");
  expect(out).not.toContain("data-theme");
});
