import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { displayMathBlocks, inlineMathSpans, type MathSpan } from "./math.helpers";

/*
 * Golden guard for the `editor` column of math-parity.golden.json — the shared
 * spec pinning how the editor's math scanners and canopy's published renderer
 * tokenize the same source (the copy here must stay byte-identical to
 * canopy's `src/math-parity.golden.json`; bump `version` and update both
 * together). Cases where `editor` and `published` differ are known,
 * documented divergences. A failure here means the editor's math semantics
 * moved — update the spec on both sides only as a deliberate decision.
 *
 * Cases tagged `editorGate` are rejected by a non-scanner gate in livePreview
 * (e.g. the Lezer code-span exclusion), which the pure scanners cannot see;
 * their editor assertion is skipped.
 */
interface GoldenCase {
  name: string;
  markdown: string;
  published: string[];
  editor: string[];
  note?: string;
  editorGate?: string;
}

const spec = JSON.parse(
  readFileSync(new URL("./math-parity.golden.json", import.meta.url), "utf8"),
) as { version: number; cases: GoldenCase[] };

/** Mirrors computeMathBlockDeco's standalone gate: the span owns its lines. */
function ownsItsLines(text: string, span: MathSpan): boolean {
  const lineStart = text.lastIndexOf("\n", span.from - 1) + 1;
  if (text.slice(lineStart, span.from).trim() !== "") return false;
  const nextBreak = text.indexOf("\n", span.to);
  const lineEnd = nextBreak === -1 ? text.length : nextBreak;
  return text.slice(span.to, lineEnd).trim() === "";
}

/**
 * Ordered math renderings the editor produces for a document, per the
 * livePreview wiring: `$$..$$` spans render as display blocks only when they
 * own their lines (mathBlockField), and the inline pass never looks inside
 * any `$$..$$` span, standalone or not.
 */
function classifyEditor(markdown: string): string[] {
  const blocks = displayMathBlocks(markdown);
  const inBlock = (pos: number) => blocks.some((b) => pos >= b.from && pos < b.to);
  const found = [
    ...blocks.filter((b) => ownsItsLines(markdown, b)).map((b) => ({ at: b.from, kind: "display" })),
    ...inlineMathSpans(markdown, inBlock).map((s) => ({ at: s.from, kind: "inline" })),
  ];
  return found.sort((a, b) => a.at - b.at).map((f) => f.kind);
}

describe("math parity golden — editor column", () => {
  for (const c of spec.cases) {
    if (c.editorGate !== undefined) continue;
    it(`${c.name}: [${c.editor.join(", ")}]`, () => {
      expect(classifyEditor(c.markdown)).toEqual(c.editor);
    });
  }
});
