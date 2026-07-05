/*
 * GFM pipe-table parsing for the live-preview editor.
 *
 * Pure scanners — no CodeMirror view or DOM dependency, so vitest imports them directly. The
 * decoration code (livePreview.ts) turns a parsed table into a rendered <table> widget on
 * inactive lines; the on-disk source is never modified.
 *
 * Cell fidelity contract (no-regression): a table line already gets inline rendering from the
 * line-level live-preview pass today — bold/em/strike/code marks, link styling, wikilink widgets,
 * inline math. The rendered cells must cover exactly that set, so replacing the block with a
 * widget never regresses what the editor already renders. Anything beyond it stays literal.
 * Cell inline syntax is recognized by the same lezer GFM grammar the editor parses with (no
 * hand-rolled inline parser); wikilinks and inline math reuse the existing pure scanners.
 */

import { GFM, parser as markdownParser } from "@lezer/markdown";
import { wikiRenderSpans } from "./wikilink.helpers";
import { escapedAt, inlineMathSpans } from "./math.helpers";

/** Column alignment from the delimiter row (`:--` / `--:` / `:-:`); null = unspecified. */
export type CellAlign = "left" | "center" | "right" | null;

/** One rendered run inside a cell. Markers hidden by the line pass are already dropped. */
export type CellSegment =
  | { kind: "text"; text: string; classes: string[] }
  | {
      kind: "wiki";
      label: string;
      target: string;
      heading: string | undefined;
      resolved: string | undefined;
    }
  | { kind: "math"; body: string };

export interface ParsedCell {
  segments: CellSegment[];
  /** Offset of the cell's content start, relative to the table source start (click-to-cell). */
  srcOffset: number;
}

export interface ParsedTable {
  header: ParsedCell[];
  align: CellAlign[];
  rows: ParsedCell[][];
  /** Identity for widget eq(): source text + wikilink resolution state. */
  key: string;
}

/** Standalone parser over the same GFM grammar the editor uses (cell inline recognition). */
const cellParser = markdownParser.configure(GFM);

/** Inline nodes that map to the live-preview mark classes (same classes as the line pass). */
const STYLE_NODES: Record<string, string> = {
  StrongEmphasis: "cm-lp-strong",
  Emphasis: "cm-lp-em",
  Strikethrough: "cm-lp-strike",
  InlineCode: "cm-lp-code",
  Link: "cm-lp-link",
};

/** Marker nodes hidden in rendered cells (mirrors the line pass's MARKER_NODES inline subset). */
const HIDE_NODES = new Set(["EmphasisMark", "CodeMark", "StrikethroughMark", "LinkMark"]);

interface RawCell {
  /** Cell text between pipes, untrimmed. */
  raw: string;
  /** Offset of `raw` within the row line. */
  start: number;
}

/** Split one row line on unescaped pipes; drops the blank edge cells of leading/trailing pipes. */
function splitRow(line: string): RawCell[] {
  const cells: RawCell[] = [];
  let cellStart = 0;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === "|" && !escapedAt(line, i)) {
      cells.push({ raw: line.slice(cellStart, i), start: cellStart });
      cellStart = i + 1;
    }
  }
  cells.push({ raw: line.slice(cellStart), start: cellStart });
  // A leading pipe produces a blank first cell; a trailing pipe a blank last cell (GFM optional pipes).
  if (cells.length > 1 && cells[0].raw.trim() === "" && line.trimStart().startsWith("|")) cells.shift();
  if (cells.length > 1 && cells[cells.length - 1].raw.trim() === "" && line.trimEnd().endsWith("|"))
    cells.pop();
  return cells;
}

/** Alignment of one delimiter cell, or undefined when the cell is not a valid delimiter. */
function delimiterAlign(raw: string): CellAlign | undefined {
  const m = /^(:?)-+(:?)$/.exec(raw.trim());
  if (!m) return undefined;
  if (m[1] && m[2]) return "center";
  if (m[2]) return "right";
  if (m[1]) return "left";
  return null;
}

/**
 * Segment a cell's (unescaped) content. Priority mirrors the line pass: wikilinks win over math,
 * both win over inline styles. Styles come from a standalone lezer parse of the cell string;
 * only inline nodes apply — block-level syntax (`# x`) stays literal, matching how a table line
 * is rendered today (no headings inside cells).
 */
function cellSegments(
  text: string,
  resolve: (target: string) => string | undefined,
  resolutionLog: string[],
): CellSegment[] {
  if (text === "") return [];
  const wiki = wikiRenderSpans(text, resolve, () => false);
  for (const w of wiki) resolutionLog.push(w.resolved ?? " ");
  const math = inlineMathSpans(text, () => false).filter(
    (m) => !wiki.some((w) => m.from < w.to && w.from < m.to),
  );

  // Char-level class/hide maps from the lezer parse (skipped inside wiki/math spans below).
  const classes: string[][] = Array.from({ length: text.length }, () => []);
  const hidden: boolean[] = new Array(text.length).fill(false);
  cellParser.parse(text).iterate({
    enter: (node) => {
      const cls = STYLE_NODES[node.name];
      if (cls !== undefined) for (let i = node.from; i < node.to; i++) classes[i].push(cls);
      if (HIDE_NODES.has(node.name)) for (let i = node.from; i < node.to; i++) hidden[i] = true;
      // Hide only the url of an inline link [text](url) — same rule as the line pass.
      if (node.name === "URL" && text[node.from - 1] === "(")
        for (let i = node.from; i < node.to; i++) hidden[i] = true;
    },
  });

  const out: CellSegment[] = [];
  let run = "";
  let runClasses: string[] = [];
  const flush = () => {
    if (run !== "") out.push({ kind: "text", text: run, classes: runClasses });
    run = "";
  };
  let i = 0;
  while (i < text.length) {
    const w = wiki.find((s) => s.from === i);
    if (w !== undefined) {
      flush();
      out.push({ kind: "wiki", label: w.label, target: w.target, heading: w.heading, resolved: w.resolved });
      i = w.to;
      continue;
    }
    const m = math.find((s) => s.from === i);
    if (m !== undefined) {
      flush();
      out.push({ kind: "math", body: m.body });
      i = m.to;
      continue;
    }
    if (!hidden[i]) {
      const cls = classes[i];
      if (run === "" || cls.join(" ") !== runClasses.join(" ")) {
        flush();
        runClasses = cls.slice();
      }
      run += text[i];
    }
    i++;
  }
  flush();
  return out;
}

/** Parse one cell: trim, unescape `\|`, segment. srcOffset points at the trimmed content start. */
function parseCell(
  cell: RawCell,
  lineOffset: number,
  resolve: (target: string) => string | undefined,
  resolutionLog: string[],
): ParsedCell {
  const leading = cell.raw.length - cell.raw.trimStart().length;
  const content = cell.raw.trim().replace(/\\\|/g, "|");
  return {
    segments: cellSegments(content, resolve, resolutionLog),
    srcOffset: lineOffset + cell.start + leading,
  };
}

/**
 * Parse a GFM pipe table's source (the syntaxTree `Table` node's line-snapped text) into
 * renderable structure. Returns null when the shape is not a valid table (malformed delimiter
 * row, column mismatch) — the caller keeps the source raw, which is the safe rendering.
 */
export function parseTable(
  text: string,
  resolve: (target: string) => string | undefined,
): ParsedTable | null {
  const lines = text.split("\n");
  if (lines.length < 2) return null;

  const headerCells = splitRow(lines[0]);
  const delimCells = splitRow(lines[1]);
  if (delimCells.length !== headerCells.length) return null;
  const align: CellAlign[] = [];
  for (const d of delimCells) {
    const a = delimiterAlign(d.raw);
    if (a === undefined) return null;
    align.push(a);
  }

  const resolutionLog: string[] = [];
  let offset = 0; // running offset of the current line within `text`
  const header = headerCells.map((c) => parseCell(c, offset, resolve, resolutionLog));

  offset += lines[0].length + 1 + lines[1].length + 1;
  const rows: ParsedCell[][] = [];
  for (let n = 2; n < lines.length; n++) {
    const line = lines[n];
    if (line.trim() === "") {
      offset += line.length + 1;
      continue;
    }
    const cells = splitRow(line).slice(0, headerCells.length);
    const row = cells.map((c) => parseCell(c, offset, resolve, resolutionLog));
    while (row.length < headerCells.length) row.push({ segments: [], srcOffset: offset });
    rows.push(row);
    offset += line.length + 1;
  }

  // NUL-separated so adjacent resolution entries cannot concatenate into a colliding key.
  return { header, align, rows, key: `${text} ${resolutionLog.join("\u0000")}` };
}
