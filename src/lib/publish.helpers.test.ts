import { expect, test } from "vitest";
import { toPublishTokens } from "./publish.helpers";

test("states the dark values for a system that prefers dark and for a page switched to dark", () => {
  const css = ':root {\n  --c: white;\n}\n[data-theme="dark"] {\n  --c: black;\n}\n';
  const out = toPublishTokens(css);
  // Light :root is the default and untouched.
  expect(out).toContain(":root {\n  --c: white;\n}");
  // The system's preference applies unless the reader switched the page to light.
  expect(out).toContain('@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {\n  --c: black;\n}');
  // A page switched to dark with its toggle gets them too.
  expect(out).toContain('[data-theme="dark"] {\n  --c: black;\n}');
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
  expect(out.split("--bg:#111").length - 1).toBe(2);
});
