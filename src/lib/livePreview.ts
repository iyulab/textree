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
import { IterMode } from "@lezer/common";
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
import { wikiRenderSpans } from "./wikilink.helpers";
import { inlineMathSpans, displayMathBlocks } from "./math.helpers";
import { renderMath } from "./mathRender";

/**
 * Reading mode flag. When set, the editor renders as a clean reading view: all markdown markers
 * are hidden (no "active line" reveal) and the frontmatter is folded regardless of cursor. The
 * editor is also made read-only by the host. Default false (normal live-preview editing).
 */
export const readingMode = Facet.define<boolean, boolean>({
  combine: (vals) => (vals.length ? vals[vals.length - 1] : false),
});

/** Resolve a wikilink target to a vault-relative note path, undefined when unresolved. */
type WikiResolve = (target: string) => string | undefined;

/**
 * Wikilink resolver facet. The host reconfigures it (via a compartment) whenever the vault tree
 * changes, so `[[links]]` render as resolved (link-styled, navigable) or unresolved (muted).
 * Default resolves nothing — every link renders unresolved until the host wires the tree.
 */
export const wikiResolver = Facet.define<WikiResolve, WikiResolve>({
  combine: (vals) => (vals.length ? vals[vals.length - 1] : () => undefined),
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
 * Wikilink widget — replaces `[[target|alias]]` source with its label on inactive lines, styled as
 * a link. Resolved links carry the destination path in `data-wikilink-path` (read by the editor's
 * click handler); unresolved links render muted with no path. The on-disk source is never modified.
 */
class WikiLinkWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly target: string,
    readonly heading: string | undefined,
    readonly resolved: string | undefined,
    readonly embed: boolean,
  ) {
    super();
  }
  eq(other: WikiLinkWidget) {
    return (
      other.label === this.label &&
      other.target === this.target &&
      other.heading === this.heading &&
      other.resolved === this.resolved &&
      other.embed === this.embed
    );
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className =
      this.resolved === undefined ? "cm-lp-wikilink cm-lp-wikilink-unresolved" : "cm-lp-wikilink";
    el.textContent = this.label;
    el.dataset.wikilinkTarget = this.target;
    if (this.heading) el.dataset.wikilinkHeading = this.heading;
    if (this.resolved !== undefined) el.dataset.wikilinkPath = this.resolved;
    el.title = this.resolved ? this.resolved : `Unresolved note: ${this.target}`;
    return el;
  }
  ignoreEvent() {
    return false;
  }
}

/**
 * Click-to-edit affordance shared by both math widgets: place the cursor at the widget's position
 * so the line/block becomes active and the raw `$..$`/`$$..$$` source is revealed (same affordance
 * as the frontmatter pill). Paired with `ignoreEvent() { return true; }` on the widgets — the
 * handler fully owns cursor placement, so CM's own mousedown handling must not also run.
 */
function attachMathClickToEdit(view: EditorView, el: HTMLElement): void {
  el.addEventListener("mousedown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    view.dispatch({ selection: { anchor: view.posAtDOM(el) } });
    view.focus();
  });
}

/**
 * Inline math widget — replaces `$..$` source with rendered KaTeX on inactive lines. The on-disk
 * source is never modified; clicking (moving the cursor onto the line) reveals the raw `$..$`.
 */
class MathInlineWidget extends WidgetType {
  constructor(readonly body: string) {
    super();
  }
  eq(other: MathInlineWidget) {
    return other.body === this.body;
  }
  toDOM(view: EditorView): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-math-inline";
    el.innerHTML = renderMath(this.body, false);
    attachMathClickToEdit(view, el);
    return el;
  }
  // Intentionally true (unlike CheckboxWidget/FrontmatterWidget which return false): the mousedown
  // handler above fully owns cursor placement. Letting CM also process widget events (return false)
  // makes its own selection logic fight the handler, so a real click no longer reveals the source.
  ignoreEvent() {
    return true;
  }
}

/**
 * Display math widget — replaces a `$$..$$` block with rendered KaTeX (centered). Multi-line, so it
 * is a block-replace decoration provided by a StateField (CM forbids block decorations from plugins,
 * same constraint as the frontmatter fold). The source is revealed when the cursor enters the block.
 */
class MathBlockWidget extends WidgetType {
  constructor(readonly body: string) {
    super();
  }
  eq(other: MathBlockWidget) {
    return other.body === this.body;
  }
  toDOM(view: EditorView): HTMLElement {
    const el = document.createElement("div");
    el.className = "cm-lp-math-block";
    el.innerHTML = renderMath(this.body, true);
    attachMathClickToEdit(view, el);
    return el;
  }
  // Intentionally true — see MathInlineWidget.ignoreEvent: the handler owns cursor placement, and
  // returning false would let CM's own selection logic undo the click-to-reveal.
  ignoreEvent() {
    return true;
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

/**
 * Block-replace decorations for display math. A `$$..$$` block is rendered only when it owns its
 * lines (opening `$$` after leading whitespace, closing `$$` at line end) — otherwise the source is
 * left raw so surrounding text is never swallowed. In reading mode every block renders; while
 * editing, a block whose lines the cursor/selection touches shows its raw source.
 */
function computeMathBlockDeco(state: EditorState): DecorationSet {
  const reading = state.facet(readingMode);
  const bodyStart = frontmatterBodyStart(state);
  const blocks = displayMathBlocks(state.doc.toString());
  if (!blocks.length) return Decoration.none;
  // Code fences/blocks/spans are not math: `$$` inside them must stay raw (mirrors the inline path's
  // codeRanges gate and the published site, where remark-math never processes math inside code). The
  // tree is read whole-document; for note-sized docs CM parses synchronously so it is complete. A
  // fence past an unparsed tail would momentarily miss this gate and render until the next edit/
  // selection recompute — the same tree-freshness limitation the ViewPlugin's decorations already have.
  const codeRanges: [number, number][] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name === "InlineCode" || node.name === "FencedCode" || node.name === "CodeBlock")
        codeRanges.push([node.from, node.to]);
    },
  });
  const ranges: Range<Decoration>[] = [];
  for (const b of blocks) {
    if (b.from < bodyStart) continue; // inside the leading frontmatter block (owned by frontmatterField)
    if (codeRanges.some(([cf, ct]) => b.from >= cf && b.from < ct)) continue; // inside code — not math
    const startLine = state.doc.lineAt(b.from);
    const endLine = state.doc.lineAt(b.to);
    // Standalone display block: nothing but whitespace before the opening / after the closing $$.
    const opensLine = state.doc.sliceString(startLine.from, b.from).trim() === "";
    const closesLine = state.doc.sliceString(b.to, endLine.to).trim() === "";
    if (!opensLine || !closesLine) continue;
    // Reveal raw when the cursor sits on any line of the block (unless reading mode).
    const cursorInside =
      !reading &&
      state.selection.ranges.some((r) => r.from <= endLine.to && r.to >= startLine.from);
    if (cursorInside) continue;
    // Mirror frontmatterBodyStart: extend to the next line's start (swallow the trailing line break)
    // unless this block ends the document. Block-replace boundaries must land on line boundaries.
    const to =
      endLine.number < state.doc.lines ? state.doc.line(endLine.number + 1).from : endLine.to;
    ranges.push(
      Decoration.replace({ block: true, widget: new MathBlockWidget(b.body) }).range(
        startLine.from,
        to,
      ),
    );
  }
  return Decoration.set(ranges, true);
}

const mathBlockField = StateField.define<DecorationSet>({
  create: computeMathBlockDeco,
  update(value, tr) {
    if (
      tr.docChanged ||
      tr.selection ||
      tr.startState.facet(readingMode) !== tr.state.facet(readingMode)
    ) {
      return computeMathBlockDeco(tr.state);
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

  // Pre-pass: collect code spans/blocks. `[[x]]` inside `code` is literal, so the wikilink pass
  // skips it. Gathered before the main pass so the link decisions below are final.
  const codeRanges: [number, number][] = [];
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        if (node.name === "InlineCode" || node.name === "FencedCode" || node.name === "CodeBlock")
          codeRanges.push([node.from, node.to]);
      },
    });
  }

  // Display math block ranges (whole document). Inline math and the main markdown pass both skip
  // inside these — the block field (mathBlockField) owns rendering; here we only need the boundaries.
  const displayBlocks = displayMathBlocks(state.doc.toString());
  const inDisplayBlock = (pos: number): boolean =>
    displayBlocks.some((b) => pos >= b.from && pos < b.to);

  // Decide which wikilinks render as widgets, resolved against the live tree. On the cursor's line,
  // inside code, or within folded frontmatter the raw source is kept instead. Computed before the
  // main pass so markdown *inside* a rendered link is left undecorated — its marker-hide replaces
  // would otherwise overlap the link's replace widget (e.g. `[[a **b** c]]`) and break the RangeSet.
  const resolve = state.facet(wikiResolver);
  const wikiSpans: { from: number; to: number; widget: WikiLinkWidget }[] = [];
  for (const { from, to } of view.visibleRanges) {
    const text = state.doc.sliceString(from, to);
    const isExcluded = (sFrom: number): boolean => {
      const absFrom = from + sFrom;
      if (fmFolded && absFrom < bodyStart) return true;
      if (active.has(state.doc.lineAt(absFrom).number)) return true;
      for (const [cf, ct] of codeRanges) if (absFrom >= cf && absFrom < ct) return true;
      if (inDisplayBlock(absFrom)) return true;
      return false;
    };
    for (const span of wikiRenderSpans(text, resolve, isExcluded)) {
      wikiSpans.push({
        from: from + span.from,
        to: from + span.to,
        widget: new WikiLinkWidget(span.label, span.target, span.heading, span.resolved, span.embed),
      });
    }
  }
  // True when a position sits inside a rendered wikilink — its inner markdown is not decorated.
  const insideWiki = (pos: number): boolean => wikiSpans.some((w) => pos >= w.from && pos < w.to);

  // Inline math spans -> rendered on inactive lines. Excluded on the cursor line, inside code,
  // inside folded frontmatter, or inside a display block (same policy as wikilinks).
  const mathSpans: { from: number; to: number; body: string }[] = [];
  for (const { from, to } of view.visibleRanges) {
    const text = state.doc.sliceString(from, to);
    const isExcluded = (sFrom: number): boolean => {
      const absFrom = from + sFrom;
      if (fmFolded && absFrom < bodyStart) return true;
      if (active.has(state.doc.lineAt(absFrom).number)) return true;
      for (const [cf, ct] of codeRanges) if (absFrom >= cf && absFrom < ct) return true;
      return inDisplayBlock(absFrom);
    };
    for (const s of inlineMathSpans(text, isExcluded)) {
      const mf = from + s.from;
      const mt = from + s.to;
      // Wiki takes precedence: a `$` inside a [[...]] is part of the link, not math. Skipping
      // overlapping math spans keeps the two replace-widget sets non-overlapping (RangeSet integrity).
      if (wikiSpans.some((w) => mf < w.to && w.from < mt)) continue;
      mathSpans.push({ from: mf, to: mt, body: s.body });
    }
  }
  const insideMath = (pos: number): boolean =>
    mathSpans.some((m) => pos >= m.from && pos < m.to);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      // Ignore mounted sub-grammars: with `codeLanguages` wired, a ```md fence mounts a nested
      // markdown grammar whose inner Emphasis/HeaderMark/… would otherwise be styled/hidden as prose
      // inside the code block. The fence's own base nodes (CodeMark/CodeInfo) are unmounted and still
      // enumerate, so fence-marker behavior is unchanged. Code coloring comes from syntaxHighlighting,
      // which reads mounts on its own pass.
      mode: IterMode.IgnoreMounts,
      enter: (node) => {
        // Skip nodes inside the folded frontmatter block — the block-replace decoration owns that range.
        if (fmFolded && node.from < bodyStart) return;
        // Skip nodes inside a rendered wikilink — the link's widget owns that range.
        if (insideWiki(node.from)) return;
        // Skip nodes inside rendered math — the math widget owns that range (inline or display block).
        if (insideMath(node.from) || inDisplayBlock(node.from)) return;
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

  // Append the wikilink widgets (computed up front). `[[note]]` is not markdown, so the syntax tree
  // never yields it — these come from a textual scan and replace the source with a clickable label.
  for (const w of wikiSpans) {
    ranges.push(Decoration.replace({ widget: w.widget }).range(w.from, w.to));
  }

  // Append inline math widgets (computed up front). `$..$` is not markdown syntax, so it comes from
  // a textual scan and replaces the source with rendered KaTeX.
  for (const m of mathSpans) {
    ranges.push(Decoration.replace({ widget: new MathInlineWidget(m.body) }).range(m.from, m.to));
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
        u.startState.facet(readingMode) !== u.state.facet(readingMode) ||
        u.startState.facet(wikiResolver) !== u.state.facet(wikiResolver)
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
  // Wikilinks: resolved = accent link; unresolved = muted with a dotted underline (the note is
  // missing, but the link is still typed/navigable-to-create later).
  ".cm-lp-wikilink": { color: "var(--accent)", textDecoration: "underline", cursor: "pointer" },
  ".cm-lp-wikilink-unresolved": {
    color: "var(--text-muted)",
    textDecoration: "underline dotted",
  },
  ".cm-lp-quote": {
    borderLeft: "3px solid var(--border-strong)",
    paddingLeft: "var(--sp-3)",
    color: "var(--text-muted)",
  },
  ".cm-lp-checkbox": { cursor: "pointer", marginRight: "0.4em", verticalAlign: "middle" },
  ".cm-lp-math-inline": { cursor: "default" },
  ".cm-lp-math-block": {
    display: "block",
    textAlign: "center",
    padding: "var(--sp-3) 0",
    overflowX: "auto",
  },
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
export const livePreview = [frontmatterField, mathBlockField, livePreviewPlugin, lpTheme];
