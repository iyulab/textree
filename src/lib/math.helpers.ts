/*
 * Math delimiter scanning for the live-preview editor (parity with canopy's remark-math).
 *
 * Pure string scanners — no CodeMirror or DOM dependency, so vitest imports them directly.
 * The editor's decoration code (livePreview.ts) turns these spans into rendered KaTeX widgets.
 *
 * Inline `$..$` is a deliberately conservative subset of remark-math: the extra guards (no space
 * adjacent to a delimiter; a closing `$` followed by a digit is not a delimiter) reject spans that
 * remark-math itself would render — probed 2026-07-05, the published pipeline renders `$5 and $10`
 * and `$ x$` as math. The editor trades that sliver of parity for zero currency false positives.
 * Escaping follows backslash-run parity (`\$` escapes, `\\$` is a literal backslash then math).
 * Display `$$..$$` may span multiple lines; an unterminated `$$` yields no span (source stays raw).
 */

/** True when the char at `i` is escaped: preceded by an odd-length run of backslashes. */
export function escapedAt(text: string, i: number): boolean {
  let n = 0;
  while (i - 1 - n >= 0 && text[i - 1 - n] === "\\") n++;
  return n % 2 === 1;
}

export interface MathSpan {
  /** Offset of the opening delimiter. */
  from: number;
  /** Offset just past the closing delimiter. */
  to: number;
  /** LaTeX source between the delimiters (delimiters excluded). */
  body: string;
}

/**
 * Inline `$..$` spans on single lines. `$$` is left for `displayMathBlocks`. A span whose opening
 * offset is `isExcluded` (cursor line, code span, folded frontmatter, display block) is dropped.
 */
export function inlineMathSpans(
  text: string,
  isExcluded: (from: number) => boolean,
): MathSpan[] {
  const out: MathSpan[] = [];
  let i = 0;
  while (i < text.length) {
    if (text[i] !== "$") {
      i++;
      continue;
    }
    // Escaped \$ is not a delimiter (backslash-run parity: \\$ is a literal backslash then math).
    if (escapedAt(text, i)) {
      i++;
      continue;
    }
    // $$ is a display delimiter, not inline.
    if (text[i + 1] === "$") {
      i += 2;
      continue;
    }
    // Opening guard: the char after `$` must exist and not be whitespace.
    const next = text[i + 1];
    if (next === undefined || /\s/.test(next)) {
      i++;
      continue;
    }
    // Find the closing `$` on the same line.
    let j = i + 1;
    let found = -1;
    while (j < text.length) {
      const c = text[j];
      if (c === "\n") break; // inline math stays on one line
      if (c === "$" && !escapedAt(text, j)) {
        const prev = text[j - 1];
        const after = text[j + 1];
        // Closing guard: no whitespace before, and not immediately followed by a digit.
        if (!/\s/.test(prev) && !(after !== undefined && /[0-9]/.test(after))) {
          found = j;
        }
        break; // first candidate decides (valid or not)
      }
      j++;
    }
    if (found > i + 1 && !isExcluded(i)) {
      out.push({ from: i, to: found + 1, body: text.slice(i + 1, found) });
      i = found + 1;
      continue;
    }
    i++;
  }
  return out;
}

/** Display `$$..$$` blocks (may span newlines). Unterminated `$$` stops the scan (rest stays raw). */
export function displayMathBlocks(text: string): MathSpan[] {
  const out: MathSpan[] = [];
  let i = 0;
  while (i < text.length - 1) {
    if (text[i] === "$" && text[i + 1] === "$" && !escapedAt(text, i)) {
      let j = i + 2;
      let found = -1;
      while (j < text.length - 1) {
        if (text[j] === "$" && text[j + 1] === "$" && !escapedAt(text, j)) {
          found = j;
          break;
        }
        j++;
      }
      if (found >= i + 2) {
        out.push({ from: i, to: found + 2, body: text.slice(i + 2, found) });
        i = found + 2;
        continue;
      }
      break; // unterminated
    }
    i++;
  }
  return out;
}
