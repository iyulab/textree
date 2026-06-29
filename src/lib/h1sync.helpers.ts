/*
 * H1 → filename sync helpers (T4) — pure functions (vitest target, no runes/IPC).
 *
 * Safe sync (C70): the first H1 of an *unnamed* note drives its filename, on an explicit edit
 * boundary only. Never the reverse (filename → body) and never on open/background. These helpers
 * decide "is this note still unnamed" and "what filename does its first H1 imply"; the rename
 * itself is the Rust `rename_note_unique` command, the final authority on validity and collisions.
 */
import { parseFrontmatter } from "./frontmatter.helpers";

/**
 * Extract the first ATX level-1 heading (`# Title`) from the note body, or null.
 * Frontmatter is skipped; fenced code blocks are ignored so a `# comment` inside a code block is
 * not mistaken for the title. Only level-1 ATX headings count (setext `===` is out of scope).
 */
export function extractFirstH1(content: string): string | null {
  const body = parseFrontmatter(content).body;
  let inFence = false;
  let fenceChar = "";
  for (const line of body.split(/\r?\n/)) {
    const fence = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fence) {
      const ch = fence[1][0];
      if (!inFence) {
        inFence = true;
        fenceChar = ch;
      } else if (ch === fenceChar) {
        inFence = false;
        fenceChar = "";
      }
      continue;
    }
    if (inFence) continue;
    // Level-1 ATX: 0-3 leading spaces, one `#`, at least one space, content, optional closing #s
    // (a closing sequence must be space-separated, so "# C#" keeps the trailing hash).
    const m = /^ {0,3}#[ \t]+(.+?)(?:[ \t]+#+)?[ \t]*$/.exec(line);
    if (m) {
      const text = m[1].trim();
      return text.length ? text : null;
    }
  }
  return null;
}

/**
 * True when the filename stem is still the default "Untitled" / "Untitled (n)" — i.e. the note
 * has not been given a real name yet. Matches the `unique_in` parenthesized format. Accepts a
 * bare stem or a `.md` filename.
 */
export function isUnnamed(filename: string): boolean {
  const stem = filename.replace(/\.md$/i, "");
  return /^Untitled( \(\d+\))?$/.test(stem);
}

/**
 * Convert heading text into a safe filename stem: strip filesystem-reserved characters and
 * leading dots, collapse whitespace, and trim trailing dots/spaces. Returns "" when nothing
 * usable remains (the caller then skips the rename). The Rust boundary (`is_valid_name`) stays
 * the final authority — this only spares it the common Windows-invalid characters up front.
 */
export function sanitizeForFilename(text: string): string {
  return text
    .replace(/[<>:"/\\|?*]/g, " ") // Windows-reserved characters -> space
    .replace(/\s+/g, " ")
    .replace(/^\.+/, "")
    .replace(/[. ]+$/, "")
    .trim();
}
