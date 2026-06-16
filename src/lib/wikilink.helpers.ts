/*
 * Wikilink parsing and resolution (Obsidian-compatible).
 *
 * Mirrors the canopy publish renderer (canopy/src/wikilink.ts, canopy/src/links.ts) so a vault
 * links the same way in the editor and in a published site — same syntax, same deterministic
 * conflict resolution. Pure functions only (vitest target); editor/runes wiring lives elsewhere.
 *
 * Supports `[[target]]`, `[[target|alias]]`, `[[target#heading]]`, `[[target#heading|alias]]`, and
 * the embed prefix `![[target]]`. Block anchors (`[[note#^id]]`) keep the `^id` as a heading
 * fragment (no per-document block index yet — matches canopy).
 */

/** The parsed parts of a `[[...]]` inner string. */
export interface WikiLink {
  /** Note name or path reference; empty for a same-document heading link (`[[#heading]]`). */
  target: string;
  /** Heading fragment after `#`, if any. */
  heading: string | undefined;
  /** Display alias after `|`, if any. */
  alias: string | undefined;
}

/** A wikilink found in a document, with its source span and rendered label. */
export interface WikiLinkSpan extends WikiLink {
  /** Start offset of the full match (including any `!` embed prefix) in the document. */
  from: number;
  /** End offset (exclusive), just past the closing `]]`. */
  to: number;
  /** True for an embed `![[...]]` (transclusion) rather than a plain link. */
  embed: boolean;
  /** Text shown in place of the source: alias when present, else target (or `#heading`). */
  label: string;
}

/**
 * Parse the inside of a `[[...]]` wikilink into its parts. Pipe (alias) is split before hash
 * (heading) so `[[a#b|c]]` yields target=a, heading=b, alias=c. Whitespace is trimmed; empty
 * components become undefined.
 */
export function parseWikiTarget(inner: string): WikiLink {
  let rest = inner.trim();

  let alias: string | undefined;
  const pipe = rest.indexOf("|");
  if (pipe !== -1) {
    alias = rest.slice(pipe + 1).trim() || undefined;
    rest = rest.slice(0, pipe).trim();
  }

  let heading: string | undefined;
  const hash = rest.indexOf("#");
  if (hash !== -1) {
    heading = rest.slice(hash + 1).trim() || undefined;
    rest = rest.slice(0, hash).trim();
  }

  return { target: rest, heading, alias };
}

/**
 * Default display text for a wikilink — mirrors canopy's `display` rule so editor and published
 * site agree: alias wins; otherwise the target; for a same-document heading link, `#heading`.
 */
export function wikiLabel(link: WikiLink): string {
  return link.alias ?? (link.target || (link.heading ? `#${link.heading}` : ""));
}

// `[[...]]` with an optional `!` embed prefix, capturing the inner text. The inner class forbids
// `[`/`]`/newline so nested brackets and line breaks cannot run a link past its intended bounds.
const WIKILINK = /(!?)\[\[([^[\]\n]+)\]\]/g;

/**
 * Find every `[[...]]` (and `![[...]]`) in a document, returning each with its source span and
 * parsed parts. A blank inner (`[[]]` / `[[ ]]`) is skipped — it is not a link.
 *
 * This is a raw textual scan; excluding matches inside code spans/fences is the caller's job (the
 * editor uses the syntax tree). Mirrors canopy's `WIKILINK` regex, plus span offsets and the embed
 * flag the editor needs.
 */
export function findWikiLinks(doc: string): WikiLinkSpan[] {
  const spans: WikiLinkSpan[] = [];
  WIKILINK.lastIndex = 0;
  let m: RegExpExecArray | null = WIKILINK.exec(doc);
  while (m !== null) {
    const inner = m[2];
    if (inner.trim() !== "") {
      const parsed = parseWikiTarget(inner);
      spans.push({
        ...parsed,
        from: m.index,
        to: m.index + m[0].length,
        embed: m[1] === "!",
        label: wikiLabel(parsed),
      });
    }
    m = WIKILINK.exec(doc);
  }
  return spans;
}

/** A wikilink ready to render in the editor: its span, label, and resolution. */
export interface WikiRenderSpan {
  from: number;
  to: number;
  /** Display text. */
  label: string;
  /** Raw target (note name/path); empty for a same-document heading link. */
  target: string;
  /** Heading fragment after `#`, if any (for in-page scroll on click). */
  heading: string | undefined;
  /**
   * Resolved vault-relative note path, or undefined when unresolved. The empty string marks a
   * same-document link (`[[#heading]]`) — resolved (styled as a link) but with no target note.
   */
  resolved: string | undefined;
  embed: boolean;
}

/**
 * Select the wikilinks in `doc` that should be rendered as link widgets, resolving each target.
 * `isExcluded(from, to)` lets the caller skip spans that must show their raw source — inside code,
 * on the cursor's line, or within a folded frontmatter block. Pure: the editor maps the result to
 * CodeMirror decorations, keeping decoration policy testable without a live view.
 */
export function wikiRenderSpans(
  doc: string,
  resolve: (target: string) => string | undefined,
  isExcluded: (from: number, to: number) => boolean,
): WikiRenderSpan[] {
  const out: WikiRenderSpan[] = [];
  for (const span of findWikiLinks(doc)) {
    if (isExcluded(span.from, span.to)) continue;
    // Empty target = same-document heading link: resolved (link-styled) with no destination note.
    const resolved = span.target === "" ? "" : resolve(span.target);
    out.push({
      from: span.from,
      to: span.to,
      label: span.label,
      target: span.target,
      heading: span.heading,
      resolved,
      embed: span.embed,
    });
  }
  return out;
}

/** A note that links to another note, surfaced in the backlinks panel. */
export interface Backlink {
  /** Vault-relative path of the source note that links here. */
  from: string;
  /** The link's display label as written (alias, target, or `#heading`). */
  label: string;
}

/** Reverse link index: which notes link to a given note. */
export interface BacklinkIndex {
  /** Source notes linking to `path`, deduped by source and sorted by path. Empty when none. */
  to(path: string): Backlink[];
}

/**
 * Remove fenced and inline code so a `[[link]]` shown inside code is not mistaken for a real link.
 * Offsets are not preserved (backlink extraction needs only targets/labels, not positions); the
 * editor, which does need positions, excludes code via the syntax tree instead.
 */
function stripCode(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, "")
    .replace(/~~~[\s\S]*?~~~/g, "")
    .replace(/`[^`\n]+`/g, "");
}

/**
 * Build a reverse link index over the vault. For each note, its `[[wikilinks]]` are parsed and
 * resolved (via `resolve`) to destination paths; the edge is recorded against the destination so
 * the panel can answer "what links here". Links inside code, self-links, and unresolved targets are
 * dropped. A source that links to the same note several times appears once (first label kept).
 * Deterministic: results are sorted by source path and never depend on input order.
 *
 * `notes` are `{ path, body }` pairs (vault-relative `.md` path + raw markdown). `resolve` must map
 * targets to the same path strings used as `note.path` (i.e. built from the same note list).
 */
export function buildBacklinkIndex(
  notes: readonly { path: string; body: string }[],
  resolve: (target: string) => string | undefined,
): BacklinkIndex {
  // dest path → (source path → label). Inner map dedupes multiple links from one source.
  const reverse = new Map<string, Map<string, string>>();

  for (const note of notes) {
    for (const link of findWikiLinks(stripCode(note.body))) {
      if (link.target === "") continue; // same-document heading link — not a backlink
      const dest = resolve(link.target);
      if (dest === undefined || dest === note.path) continue; // unresolved or self-link
      let sources = reverse.get(dest);
      if (sources === undefined) {
        sources = new Map();
        reverse.set(dest, sources);
      }
      if (!sources.has(note.path)) sources.set(note.path, link.label);
    }
  }

  return {
    to(path) {
      const sources = reverse.get(path);
      if (sources === undefined) return [];
      return [...sources.entries()]
        .map(([from, label]) => ({ from, label }))
        .sort((a, b) => a.from.localeCompare(b.from));
    },
  };
}

/** A `[[` autocomplete suggestion. */
export interface WikiCompletion {
  /** Note stem shown as the option label. */
  label: string;
  /** Disambiguating path, shown only when the stem is ambiguous. */
  detail: string | undefined;
  /** Resolvable target to insert after `[[` (no brackets — the editor adds the closing `]]`). */
  insert: string;
}

/**
 * Suggestions for a `[[` autocomplete query. Each note becomes one option: the stem is the label,
 * and the inserted target is the stem — or the full path when the stem is ambiguous, so the picked
 * note always resolves back to itself. Filtered by `query` (case-insensitive, against stem and
 * path), prefix matches first, deterministic, capped at 50. Pure: the editor maps the result to
 * CodeMirror completions and supplies the closing `]]`.
 */
export function wikiCompletions(notePaths: readonly string[], query: string): WikiCompletion[] {
  const q = query.trim().toLowerCase();
  const stemCount = new Map<string, number>();
  const entries = notePaths.map((path) => {
    const noExt = path.replace(/\\/g, "/").replace(/\.md$/i, "");
    const stem = noExt.split("/").pop() ?? noExt;
    const lc = stem.toLowerCase();
    stemCount.set(lc, (stemCount.get(lc) ?? 0) + 1);
    return { noExt, stem, lc };
  });

  const out: WikiCompletion[] = [];
  for (const e of entries) {
    if (q && !e.lc.includes(q) && !e.noExt.toLowerCase().includes(q)) continue;
    const ambiguous = (stemCount.get(e.lc) ?? 0) > 1;
    out.push({
      label: e.stem,
      detail: ambiguous ? e.noExt : undefined,
      insert: ambiguous ? e.noExt : e.stem,
    });
  }

  out.sort((a, b) => {
    const ap = a.label.toLowerCase().startsWith(q) ? 0 : 1;
    const bp = b.label.toLowerCase().startsWith(q) ? 0 : 1;
    return ap - bp || a.insert.length - b.insert.length || a.insert.localeCompare(b.insert);
  });
  return out.slice(0, 50);
}

/** Escape a string for safe use inside a RegExp. */
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Character offset of the line that a `[[note#heading]]` fragment points to, or null when not found.
 * A plain fragment matches a markdown heading by text (case-insensitive, trimmed). A `^`-prefixed
 * fragment is an Obsidian block anchor — it matches a `^id` token on a line. Used to scroll the
 * editor to the target after opening a note.
 */
export function findHeadingOffset(doc: string, heading: string): number | null {
  const want = heading.trim();
  if (want === "") return null;
  const blockAnchor = want.startsWith("^") ? want.slice(1) : null;
  const anchorRe =
    blockAnchor !== null ? new RegExp(`(^|\\s)\\^${escapeRegExp(blockAnchor)}\\s*$`, "i") : null;

  let offset = 0;
  for (const line of doc.split("\n")) {
    if (anchorRe !== null) {
      if (anchorRe.test(line)) return offset;
    } else {
      const m = /^#{1,6}\s+(.*)$/.exec(line);
      if (m && m[1].trim().toLowerCase() === want.toLowerCase()) return offset;
    }
    offset += line.length + 1; // +1 for the consumed newline
  }
  return null;
}

/** Resolves a wikilink target to a vault-relative note path, or undefined when unresolved. */
export interface WikiResolver {
  resolve(target: string): string | undefined;
}

/** Normalize a target for matching: POSIX slashes, no leading slash, no `.md`, lowercased. */
function normalizeTarget(target: string): string {
  return target
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\.md$/i, "")
    .toLowerCase();
}

/**
 * Build a resolver over the vault's note paths. Matching is case-insensitive. A bare name (`idea`)
 * matches by stem; a path-bearing target (`notes/idea`) matches the full path. When several notes
 * share a stem, resolution is deterministic — shortest path wins, ties broken lexicographically —
 * so results never depend on input order. Mirrors canopy's `buildLinkIndex`.
 *
 * Paths are vault-relative POSIX paths to `.md` files (e.g. `notes/idea.md`, `folder/folder.md`).
 */
export function buildWikiResolver(notePaths: readonly string[]): WikiResolver {
  const byName = new Map<string, string[]>();
  const byPath = new Map<string, string>();

  for (const path of notePaths) {
    const norm = normalizeTarget(path);
    byPath.set(norm, path);
    const stem = norm.split("/").pop() ?? norm;
    const list = byName.get(stem) ?? [];
    list.push(path);
    byName.set(stem, list);
  }

  for (const list of byName.values()) {
    list.sort((a, b) => a.split("/").length - b.split("/").length || a.localeCompare(b));
  }

  return {
    resolve(target) {
      const norm = normalizeTarget(target);
      if (norm === "") return undefined;
      if (norm.includes("/")) return byPath.get(norm);
      return byName.get(norm)?.[0];
    },
  };
}
