import { describe, it, expect } from "vitest";
import { inlineMathSpans, displayMathBlocks } from "./math.helpers";

const none = () => false;

describe("inlineMathSpans", () => {
  it("matches a simple inline formula", () => {
    const spans = inlineMathSpans("a $x+1$ b", none);
    expect(spans).toEqual([{ from: 2, to: 7, body: "x+1" }]);
  });

  it("matches two formulas on one line", () => {
    const spans = inlineMathSpans("$a$ and $b$", none);
    expect(spans.map((s) => s.body)).toEqual(["a", "b"]);
  });

  it("ignores currency: closing $ followed by a digit", () => {
    expect(inlineMathSpans("I paid $5 and $10 today", none)).toEqual([]);
  });

  it("rejects a closing $ immediately followed by a digit (digit guard, no whitespace)", () => {
    // prev char is 'x' (passes the whitespace guard), so ONLY the digit-after-close guard can reject this.
    expect(inlineMathSpans("$x$5 text", none)).toEqual([]);
  });

  it("rejects whitespace right after opening $", () => {
    expect(inlineMathSpans("$ x$", none)).toEqual([]);
  });

  it("rejects whitespace right before closing $", () => {
    expect(inlineMathSpans("$x $", none)).toEqual([]);
  });

  it("rejects empty body $$ (that is display)", () => {
    expect(inlineMathSpans("$$", none)).toEqual([]);
  });

  it("does not treat $$ as two inline delimiters", () => {
    expect(inlineMathSpans("$$x$$", none)).toEqual([]);
  });

  it("respects escaped \\$ inside the body", () => {
    const spans = inlineMathSpans("$a\\$b$", none);
    expect(spans).toEqual([{ from: 0, to: 6, body: "a\\$b" }]);
  });

  it("ignores an escaped opening \\$", () => {
    expect(inlineMathSpans("cost \\$5 only", none)).toEqual([]);
  });

  // Backslash-run parity (probed against canopy's remark-math pipeline 2026-07-05):
  // a run of N backslashes escapes the delimiter iff N is odd (`\\` is an escaped backslash).
  it("treats $ after an escaped backslash (\\\\) as a delimiter", () => {
    // Source text: a \\$x$ b — published output renders math `x` after a literal backslash.
    expect(inlineMathSpans("a \\\\$x$ b", none)).toEqual([{ from: 4, to: 7, body: "x" }]);
  });

  it("keeps $ escaped after three backslashes (odd run)", () => {
    expect(inlineMathSpans("a \\\\\\$x$ b", none)).toEqual([]);
  });

  it("treats $ after four backslashes (even run) as a delimiter", () => {
    expect(inlineMathSpans("a \\\\\\\\$x$ b", none)).toEqual([{ from: 6, to: 9, body: "x" }]);
  });

  it("closes on $ after an escaped backslash (\\\\) in the body", () => {
    // $a\\$ — the closing $ follows a LaTeX line-break `\\`, not an escaped dollar.
    expect(inlineMathSpans("$a\\\\$ b", none)).toEqual([{ from: 0, to: 5, body: "a\\\\" }]);
  });

  it("does not span across a newline", () => {
    expect(inlineMathSpans("$a\nb$", none)).toEqual([]);
  });

  it("skips spans the caller excludes (e.g. code spans)", () => {
    // exclude any open offset >= 0 -> everything excluded
    expect(inlineMathSpans("$x$", (from) => from === 0)).toEqual([]);
  });
});

describe("displayMathBlocks", () => {
  it("matches a multi-line display block", () => {
    const text = "$$\n\\int_0^1 x\\,dx\n$$";
    const spans = displayMathBlocks(text);
    expect(spans).toHaveLength(1);
    expect(spans[0].from).toBe(0);
    expect(spans[0].to).toBe(text.length);
    expect(spans[0].body).toBe("\n\\int_0^1 x\\,dx\n");
  });

  it("matches a single-line display block", () => {
    expect(displayMathBlocks("$$a=b$$").map((s) => s.body)).toEqual(["a=b"]);
  });

  it("does not match an unterminated block", () => {
    expect(displayMathBlocks("$$ a = b")).toEqual([]);
  });

  it("matches two separate blocks", () => {
    const spans = displayMathBlocks("$$a$$ text $$b$$");
    expect(spans.map((s) => s.body)).toEqual(["a", "b"]);
  });

  it("ignores an escaped \\$$", () => {
    expect(displayMathBlocks("\\$$a$$")).toEqual([]);
  });

  it("treats $$ after an escaped backslash (\\\\) as an opening delimiter", () => {
    expect(displayMathBlocks("\\\\$$a$$").map((s) => s.body)).toEqual(["a"]);
  });

  it("closes on $$ after an escaped backslash (\\\\) in the body", () => {
    expect(displayMathBlocks("$$a\\\\$$").map((s) => s.body)).toEqual(["a\\\\"]);
  });
});
