/*
 * Live preview (Obsidian LP model) — CodeMirror 6 inline decorations.
 *
 * Renders markdown over the editor rather than in a separate preview panel:
 *  - Headings always apply size; emphasis/code etc. always apply style (line/mark decorations).
 *  - Markers (#, **, *, `, ~~) are hidden **only on lines without the cursor** (replace decoration).
 *    When the cursor enters that line, the markers reappear so the original markdown can be edited directly.
 *
 * Performance: iterates the syntaxTree over only the visible ranges (visibleRanges), and recomputes
 * decorations only on document, viewport, or selection changes.
 *
 * Scope (D4): headings, emphasis (bold/italic), strikethrough, inline code. Links, checkboxes,
 * quotes/horizontal rules, and inline images are follow-ups (D4 continued/P4).
 */

import { syntaxTree } from "@codemirror/language";
import { Facet, StateField, type EditorState, type Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import { parseFrontmatter } from "./frontmatter.helpers";

/**
 * Reading mode flag. When set, the editor renders as a clean reading view: all markdown markers
 * are hidden (no "active line" reveal) and the frontmatter is folded regardless of cursor. The
 * editor is also made read-only by the host. Default false (normal live-preview editing).
 */
export const readingMode = Facet.define<boolean, boolean>({
  combine: (vals) => (vals.length ? vals[vals.length - 1] : false),
});

/** Shared empty active-line set for reading mode (read-only, never mutated). */
const NO_ACTIVE_LINES: ReadonlySet<number> = new Set();

/** Task list checkbox widget — clicking toggles [ ]↔[x] in the on-disk source. */
class CheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly from: number,
    readonly to: number,
  ) {
    super();
  }
  eq(other: CheckboxWidget) {
    return other.checked === this.checked && other.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = this.checked;
    box.className = "cm-lp-checkbox";
    // Block mousedown: prevents the cursor from moving to the line (which would remove the widget) and only performs the toggle.
    box.addEventListener("mousedown", (e) => {
      e.preventDefault();
      view.dispatch({
        changes: { from: this.from, to: this.to, insert: this.checked ? "[ ]" : "[x]" },
      });
    });
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

/** Horizontal rule (---) widget — renders a horizontal line instead of the source on inactive lines. */
class HrWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM(): HTMLElement {
    const hr = document.createElement("span");
    hr.className = "cm-lp-hr";
    return hr;
  }
}

/**
 * Folded-frontmatter placeholder — replaces the leading `---` block with a compact pill while the
 * cursor is elsewhere (the title/icon already render in the page header above the editor). Clicking
 * it moves the cursor into the block so the raw YAML can be edited. The source is never modified.
 */
class FrontmatterWidget extends WidgetType {
  constructor(readonly label: string) {
    super();
  }
  eq(other: FrontmatterWidget) {
    return other.label === this.label;
  }
  toDOM(view: EditorView): HTMLElement {
    const pill = document.createElement("div");
    pill.className = "cm-lp-frontmatter";
    pill.textContent = `≡ ${this.label}`;
    pill.title = "Edit properties";
    pill.addEventListener("mousedown", (e) => {
      e.preventDefault();
      // Place the cursor on the first content line (or the opening fence) to reveal the source.
      const anchor = view.state.doc.lines >= 2 ? view.state.doc.line(2).from : 0;
      view.dispatch({ selection: { anchor }, scrollIntoView: true });
      view.focus();
    });
    return pill;
  }
  ignoreEvent() {
    return false;
  }
}

/**
 * Offset where the body begins when the document opens with a well-formed frontmatter block, else 0.
 * Mirrors the recognition contract of `parseFrontmatter` (first line `---`, closing `---` line) but
 * scans via the line API instead of materializing the whole document on every decoration rebuild.
 */
function frontmatterBodyStart(state: EditorState): number {
  const { doc } = state;
  if (doc.lines < 2 || doc.line(1).text.trimEnd() !== "---") return 0;
  for (let n = 2; n <= doc.lines; n++) {
    if (doc.line(n).text.trimEnd() === "---") {
      // Body starts after the closing fence's line break (or at the fence end if it is the last line).
      return n < doc.lines ? doc.line(n + 1).from : doc.line(n).to;
    }
  }
  return 0; // unterminated → not frontmatter (matches parseFrontmatter; source left intact)
}

/** Whether the leading frontmatter block should be folded: present and (reading mode OR cursor outside it). */
function isFrontmatterFolded(state: EditorState, bodyStart: number): boolean {
  if (bodyStart <= 0) return false;
  if (state.facet(readingMode)) return true;
  return !state.selection.ranges.some((r) => r.from < bodyStart);
}

/**
 * Block-replace decoration for a folded frontmatter block. Block decorations cannot be provided by a
 * ViewPlugin (CM constraint), so this lives in a StateField. The pill renders the field keys and
 * reveals the source on click; the source itself is never modified.
 */
function computeFrontmatterDeco(state: EditorState): DecorationSet {
  const bodyStart = frontmatterBodyStart(state);
  if (!isFrontmatterFolded(state, bodyStart)) return Decoration.none;
  const keys = Object.keys(parseFrontmatter(state.doc.sliceString(0, bodyStart)).data);
  const label = keys.length ? keys.join(", ") : "Properties";
  return Decoration.set([
    Decoration.replace({ block: true, widget: new FrontmatterWidget(label) }).range(0, bodyStart),
  ]);
}

const frontmatterField = StateField.define<DecorationSet>({
  create: computeFrontmatterDeco,
  update(value, tr) {
    // Recompute when the document, selection, or reading mode (facet) changes.
    if (
      tr.docChanged ||
      tr.selection ||
      tr.startState.facet(readingMode) !== tr.state.facet(readingMode)
    ) {
      return computeFrontmatterDeco(tr.state);
    }
    return value.map(tr.changes);
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Set of line numbers touched by the cursor/selection — these lines do not hide markers (source exposed). */
function activeLines(view: EditorView): Set<number> {
  const s = new Set<number>();
  const { doc } = view.state;
  for (const r of view.state.selection.ranges) {
    const a = doc.lineAt(r.from).number;
    const b = doc.lineAt(r.to).number;
    for (let l = a; l <= b; l++) s.add(l);
  }
  return s;
}

// Per-line heading size decorations (classes defined in lpTheme).
const headingLine = [
  null,
  Decoration.line({ class: "cm-lp-h1" }),
  Decoration.line({ class: "cm-lp-h2" }),
  Decoration.line({ class: "cm-lp-h3" }),
  Decoration.line({ class: "cm-lp-h4" }),
  Decoration.line({ class: "cm-lp-h5" }),
  Decoration.line({ class: "cm-lp-h6" }),
];

const strongMark = Decoration.mark({ class: "cm-lp-strong" });
const emMark = Decoration.mark({ class: "cm-lp-em" });
const strikeMark = Decoration.mark({ class: "cm-lp-strike" });
const codeMark = Decoration.mark({ class: "cm-lp-code" });
const linkMark = Decoration.mark({ class: "cm-lp-link" });
const quoteLine = Decoration.line({ class: "cm-lp-quote" });
const hideMark = Decoration.replace({});

// Marker nodes to hide (lezer-markdown). URL is excluded from the general hide set
// because it should be hidden only for inline links (autolink/bare url have the URL as their only visible content).
const MARKER_NODES = new Set([
  "HeaderMark",
  "EmphasisMark",
  "CodeMark",
  "StrikethroughMark",
  "LinkMark",
  "QuoteMark",
]);

function buildDecorations(view: EditorView): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const { state } = view;
  const reading = state.facet(readingMode);
  // Reading mode hides every marker (no line is ever "active"); editing mode reveals the source on
  // the cursor's line.
  const active: ReadonlySet<number> = reading ? NO_ACTIVE_LINES : activeLines(view);

  // Frontmatter folding state — the block-replace decoration itself is provided by frontmatterField
  // (CM forbids block decorations from plugins); here we only need the boundary to skip the inner
  // nodes so the plugin's inline/line decorations don't render under the folded block.
  const bodyStart = frontmatterBodyStart(state);
  const fmFolded = isFrontmatterFolded(state, bodyStart);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        // Skip nodes inside the folded frontmatter block — the block-replace decoration owns that range.
        if (fmFolded && node.from < bodyStart) return;
        const name = node.name;

        // Heading: size decoration over the whole line (size is kept even on the cursor line — same as Obsidian).
        const h = /^ATXHeading([1-6])$/.exec(name);
        if (h) {
          const deco = headingLine[Number(h[1])];
          if (deco) ranges.push(deco.range(state.doc.lineAt(node.from).from));
          return;
        }

        // Blockquote: left-bar style on each line.
        if (name === "Blockquote") {
          const startLn = state.doc.lineAt(node.from).number;
          const endLn = state.doc.lineAt(node.to).number;
          for (let l = startLn; l <= endLn; l++) {
            ranges.push(quoteLine.range(state.doc.line(l).from));
          }
          return;
        }

        // Horizontal rule (---): horizontal line widget instead of the source on inactive lines.
        if (name === "HorizontalRule") {
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          const line = state.doc.lineAt(node.from);
          if (line.from < line.to)
            ranges.push(
              Decoration.replace({ widget: new HrWidget() }).range(line.from, line.to),
            );
          return;
        }

        // Task list checkbox: replace the marker with a toggle widget on inactive lines.
        if (name === "TaskMarker") {
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          const checked = /[xX]/.test(state.doc.sliceString(node.from, node.to));
          ranges.push(
            Decoration.replace({
              widget: new CheckboxWidget(checked, node.from, node.to),
            }).range(node.from, node.to),
          );
          return;
        }

        // Inline styles: always applied to the content range.
        if (name === "StrongEmphasis") ranges.push(strongMark.range(node.from, node.to));
        else if (name === "Emphasis") ranges.push(emMark.range(node.from, node.to));
        else if (name === "Strikethrough") ranges.push(strikeMark.range(node.from, node.to));
        else if (name === "InlineCode") ranges.push(codeMark.range(node.from, node.to));
        else if (name === "Link") ranges.push(linkMark.range(node.from, node.to));
        else if (name === "URL") {
          // Hide only the url of an inline link [text](url) (preceding char is '('). For autolink/bare
          // url, the url itself is the displayed content, so preserve it.
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          if (state.doc.sliceString(node.from - 1, node.from) === "(")
            ranges.push(hideMark.range(node.from, node.to));
        } else if (MARKER_NODES.has(name)) {
          // Hide markers — but the line with the cursor shows its source.
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          let end = node.to;
          // For heading markers, also hide the one following space to remove leftover indentation.
          if (name === "HeaderMark" && state.doc.sliceString(end, end + 1) === " ") end += 1;
          if (node.from < end) ranges.push(hideMark.range(node.from, end));
        }
      },
    });
  }

  // Delegate sorting (sort=true) — sorts safely even when line/mark/replace are mixed.
  return Decoration.set(ranges, true);
}

/** Live preview ViewPlugin — holds the decorations and recomputes them on change. */
const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }
    update(u: ViewUpdate) {
      // Also rebuild when reading mode (facet) toggles — a compartment reconfigure is not a doc/
      // viewport/selection change, so it would otherwise leave markers in their previous state.
      if (
        u.docChanged ||
        u.viewportChanged ||
        u.selectionSet ||
        u.startState.facet(readingMode) !== u.state.facet(readingMode)
      ) {
        this.decorations = buildDecorations(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

/** Live preview typography — token-based (follows the theme). */
const lpTheme = EditorView.theme({
  // Headings: full modular scale + opinionated top spacing (reuses the spacing tokens) so unstyled
  // notes get section rhythm by default. Uses padding (not margin) for the gap — CodeMirror's height
  // oracle measures line boxes and ignores margins, so a top margin would desync posAtCoords and
  // misplace the cursor on click. Size is kept even on the cursor line (same as Obsidian).
  ".cm-lp-h1": { fontSize: "var(--font-size-h1)", fontWeight: "var(--font-weight-semibold)", lineHeight: "var(--line-height-tight)", paddingTop: "var(--sp-6)" },
  ".cm-lp-h2": { fontSize: "var(--font-size-h2)", fontWeight: "var(--font-weight-semibold)", lineHeight: "var(--line-height-tight)", paddingTop: "var(--sp-5)" },
  ".cm-lp-h3": { fontSize: "var(--font-size-h3)", fontWeight: "var(--font-weight-semibold)", lineHeight: "var(--line-height-tight)", paddingTop: "var(--sp-4)" },
  ".cm-lp-h4": { fontSize: "var(--font-size-h4)", fontWeight: "var(--font-weight-semibold)", paddingTop: "var(--sp-3)" },
  ".cm-lp-h5": { fontSize: "var(--font-size-h5)", fontWeight: "var(--font-weight-semibold)", color: "var(--text-muted)", paddingTop: "var(--sp-3)" },
  ".cm-lp-h6": { fontSize: "var(--font-size-h6)", fontWeight: "var(--font-weight-semibold)", color: "var(--text-muted)", paddingTop: "var(--sp-3)" },
  ".cm-lp-strong": { fontWeight: "var(--font-weight-bold)" },
  ".cm-lp-em": { fontStyle: "italic" },
  ".cm-lp-strike": { textDecoration: "line-through", color: "var(--text-muted)" },
  ".cm-lp-code": {
    fontFamily: "var(--font-monospace)",
    fontSize: "0.9em",
    background: "var(--bg-secondary-alt)",
    padding: "0.1em 0.35em",
    borderRadius: "var(--radius-s)",
  },
  ".cm-lp-link": { color: "var(--accent)", textDecoration: "underline", cursor: "pointer" },
  ".cm-lp-quote": {
    borderLeft: "3px solid var(--border-strong)",
    paddingLeft: "var(--sp-3)",
    color: "var(--text-muted)",
  },
  ".cm-lp-checkbox": { cursor: "pointer", marginRight: "0.4em", verticalAlign: "middle" },
  ".cm-lp-hr": {
    display: "inline-block",
    width: "100%",
    borderTop: "1px solid var(--border-strong)",
    verticalAlign: "middle",
  },
  ".cm-lp-frontmatter": {
    display: "inline-flex",
    alignItems: "center",
    gap: "0.3em",
    fontSize: "var(--font-size-smaller)",
    color: "var(--text-muted)",
    background: "var(--bg-secondary-alt)",
    padding: "0.15em 0.6em",
    borderRadius: "var(--radius-m)",
    cursor: "pointer",
    userSelect: "none",
  },
});

/** Live preview extension bundle to add to the editor. */
export const livePreview = [frontmatterField, livePreviewPlugin, lpTheme];
