//! Helpers for publishing the vault to a static site via canopy.
//!
//! The app's `tokens.css` themes via `:root` (light) + `[data-theme="dark"]`, toggled by JS. A
//! published page follows the reader's system preference until the reader switches it with the
//! page's own toggle, which sets `data-theme`. Injected as-is, the dark values would apply only
//! after a switch. `toPublishTokens` states them twice, the way canopy states its own: for a
//! system that prefers dark (unless the page was switched to light), and for a page switched to
//! dark.

const DARK_SELECTOR = '[data-theme="dark"]';

/**
 * Adds `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { … } }` before the
 * `[data-theme="dark"]` rule, which stays, so a published page follows the system and its toggle. CSS without a dark block is returned unchanged. Token declaration
 * blocks do not nest braces, but the matching is brace-aware so a future nested rule is safe.
 */
export function toPublishTokens(tokensCss: string): string {
  const start = tokensCss.indexOf(DARK_SELECTOR);
  if (start === -1) {
    return tokensCss;
  }
  const open = tokensCss.indexOf("{", start);
  if (open === -1) {
    return tokensCss;
  }

  let depth = 0;
  let close = -1;
  for (let i = open; i < tokensCss.length; i++) {
    const ch = tokensCss[i];
    if (ch === "{") {
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0) {
        close = i;
        break;
      }
    }
  }
  if (close === -1) {
    return tokensCss;
  }

  const inner = tokensCss.slice(open + 1, close);
  const before = tokensCss.slice(0, start);
  const rest = tokensCss.slice(start);
  const bySystem = `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {${inner}}\n}\n`;
  return before + bySystem + rest;
}
