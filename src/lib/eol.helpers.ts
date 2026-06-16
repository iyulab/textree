//! Line-ending detection for byte-faithful round-trips.
//!
//! CodeMirror's `Text.toString()` always joins lines with `\n`, so a CRLF source silently
//! becomes LF on the next edit. To keep a vault shared with another editor (e.g. Obsidian on
//! Windows) byte-lossless, we detect the source line ending on load and re-apply it to the
//! editor's output before writing it back to disk.

export type LineEnding = "\n" | "\r\n";

/** Returns the document's line ending, classified by its first line break. Defaults to LF. */
export function detectLineEnding(doc: string): LineEnding {
  const lf = doc.indexOf("\n");
  if (lf === -1) {
    return "\n";
  }
  return doc[lf - 1] === "\r" ? "\r\n" : "\n";
}

/**
 * Re-joins `text` (whose breaks are the editor's `\n`) with `eol`. A no-op for LF; for CRLF it
 * rewrites every line break to `\r\n` while tolerating any `\r\n` already present (idempotent).
 */
export function normalizeLineEndings(text: string, eol: LineEnding): string {
  if (eol === "\n") {
    return text;
  }
  return text.replace(/\r?\n/g, "\r\n");
}
