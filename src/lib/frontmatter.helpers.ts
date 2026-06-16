/*
 * Frontmatter parsing — pure helpers (vitest target, no runes/IPC).
 *
 * Recognizes a leading YAML-ish frontmatter block delimited by `---` fences, the convention
 * shared by Obsidian / Jekyll / CommonMark tooling. Deliberately a minimal scalar parser
 * (key: value, one per line) rather than a full YAML engine: the pretty-by-default page header
 * only needs scalar fields (title / icon / cover), so a dependency-free parser keeps the stack
 * minimal. If a frontmatter database view later proves real demand for lists/nesting/typing,
 * that is the point to adopt a real YAML dependency (demand-driven growth) — not before.
 *
 * Data-safety contract: an unterminated block (opening `---` with no closing fence) is NOT
 * treated as frontmatter — it is left in the body verbatim. The parser never drops or rewrites
 * source text; `body + stripped prefix` always reconstructs the original document.
 */

export interface Frontmatter {
  /** A well-formed, terminated frontmatter block was found at the document start. */
  present: boolean;
  /** Parsed scalar fields (original key case preserved). Last value wins on duplicate keys. */
  data: Record<string, string>;
  /** The document with the frontmatter block removed (whole document if none). */
  body: string;
  /** Offset in the original document where the body begins (0 if no frontmatter). */
  end: number;
}

// Leading frontmatter: first line is exactly `---` (trailing spaces allowed), a (possibly empty)
// content region, then a closing `---` line. The content+newline group is optional so an empty
// block (`---\n---`) is recognized. Anchored at string start (no `m` flag) — frontmatter is only
// valid at offset 0.
const FRONTMATTER_RE = /^---[ \t]*\r?\n(?:([\s\S]*?)\r?\n)?---[ \t]*(?:\r?\n|$)/;

/** Strip a single pair of matching surrounding quotes from a scalar value. */
function unquote(v: string): string {
  if (v.length >= 2) {
    const q = v[0];
    if ((q === '"' || q === "'") && v[v.length - 1] === q) return v.slice(1, -1);
  }
  return v;
}

/** Parse the inner content of a frontmatter block into scalar key/value pairs. */
function parseFields(content: string): Record<string, string> {
  const data: Record<string, string> = {};
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue; // blank or YAML comment
    const colon = line.indexOf(":");
    if (colon === -1) continue; // not a key: value line — ignore (minimal scalar grammar)
    const key = line.slice(0, colon).trim();
    if (!key) continue;
    data[key] = unquote(line.slice(colon + 1).trim());
  }
  return data;
}

/**
 * Parse a leading frontmatter block from a markdown document.
 * Returns `present: false` (and the whole document as `body`) when there is no well-formed block.
 */
export function parseFrontmatter(doc: string): Frontmatter {
  const m = FRONTMATTER_RE.exec(doc);
  if (!m) return { present: false, data: {}, body: doc, end: 0 };
  const end = m[0].length;
  return { present: true, data: parseFields(m[1] ?? ""), body: doc.slice(end), end };
}

/** Case-insensitive field lookup (frontmatter keys are conventionally lowercase but tolerate case). */
export function getField(
  data: Record<string, string>,
  name: string,
): string | undefined {
  const direct = data[name];
  if (direct !== undefined) return direct;
  const lower = name.toLowerCase();
  for (const key of Object.keys(data)) {
    if (key.toLowerCase() === lower) return data[key];
  }
  return undefined;
}
