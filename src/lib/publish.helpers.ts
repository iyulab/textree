//! Helpers for publishing the vault to a static site via canopy.
//!
//! The app's `tokens.css` themes via `:root` (light) + `[data-theme="dark"]`, toggled by JS. A
//! published static site has no toggle and never sets `data-theme`, so injecting those tokens
//! as-is would strand the site in light mode. `toPublishTokens` rewrites the dark block to fire on
//! the OS preference instead, so the published artifact respects light/dark like the app does.

const DARK_SELECTOR = '[data-theme="dark"]';

/**
 * Rewrites the `[data-theme="dark"]` rule into `@media (prefers-color-scheme: dark) { :root { … } }`
 * so a static site auto-themes. CSS without a dark block is returned unchanged. Token declaration
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
  const after = tokensCss.slice(close + 1);
  const wrapped = `@media (prefers-color-scheme: dark) {\n  :root {${inner}}\n}`;
  return before + wrapped + after;
}
