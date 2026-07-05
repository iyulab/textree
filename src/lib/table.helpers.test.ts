import { describe, expect, it } from "vitest";
import { parseTable, type CellSegment } from "./table.helpers";

/** Resolver stub: "target" resolves, everything else does not. */
const resolve = (t: string) => (t === "target" ? "notes/target.md" : undefined);

/** Flatten a cell's text segments into a plain string (hidden markers already dropped). */
function cellText(segments: CellSegment[]): string {
  return segments.map((s) => (s.kind === "text" ? s.text : s.kind === "wiki" ? s.label : s.body)).join("");
}

describe("parseTable — structure", () => {
  it("parses a basic 2x2 table with header and alignment row", () => {
    const t = parseTable("| Name | Value |\n| --- | --- |\n| a | b |\n| c | d |", resolve);
    expect(t).not.toBeNull();
    expect(t!.header).toHaveLength(2);
    expect(cellText(t!.header[0].segments)).toBe("Name");
    expect(t!.rows).toHaveLength(2);
    expect(cellText(t!.rows[1][1].segments)).toBe("d");
  });

  it("parses alignment: none, left, right, center", () => {
    const t = parseTable("| a | b | c | d |\n| --- | :-- | --: | :-: |\n| 1 | 2 | 3 | 4 |", resolve);
    expect(t!.align).toEqual([null, "left", "right", "center"]);
  });

  it("accepts rows without leading/trailing pipes", () => {
    const t = parseTable("a | b\n--- | ---\n1 | 2", resolve);
    expect(t!.header.map((c) => cellText(c.segments))).toEqual(["a", "b"]);
    expect(t!.rows[0].map((c) => cellText(c.segments))).toEqual(["1", "2"]);
  });

  it("keeps an escaped \\| inside a cell (renders as a literal pipe)", () => {
    const t = parseTable("| a\\|b | c |\n| --- | --- |\n| x | y |", resolve);
    expect(cellText(t!.header[0].segments)).toBe("a|b");
    expect(t!.header).toHaveLength(2);
  });

  it("pads short rows and truncates long rows to the header width (GFM)", () => {
    const t = parseTable("| a | b |\n| --- | --- |\n| only |\n| 1 | 2 | 3 |", resolve);
    expect(t!.rows[0]).toHaveLength(2);
    expect(cellText(t!.rows[0][1].segments)).toBe("");
    expect(t!.rows[1]).toHaveLength(2);
  });

  it("returns null when the delimiter row is malformed", () => {
    expect(parseTable("| a | b |\n| --- | xx |\n| 1 | 2 |", resolve)).toBeNull();
  });

  it("returns null when the delimiter column count mismatches the header", () => {
    expect(parseTable("| a | b |\n| --- |\n| 1 | 2 |", resolve)).toBeNull();
  });

  it("returns null for fewer than two lines", () => {
    expect(parseTable("| a | b |", resolve)).toBeNull();
  });
});

describe("parseTable — srcOffset (click-to-cell mapping)", () => {
  it("maps each cell's content start to its offset in the table source", () => {
    const src = "| ab | cd |\n| --- | --- |\n| ef | gh |";
    const t = parseTable(src, resolve)!;
    expect(src.slice(t.header[0].srcOffset, t.header[0].srcOffset + 2)).toBe("ab");
    expect(src.slice(t.header[1].srcOffset, t.header[1].srcOffset + 2)).toBe("cd");
    expect(src.slice(t.rows[0][0].srcOffset, t.rows[0][0].srcOffset + 2)).toBe("ef");
    expect(src.slice(t.rows[0][1].srcOffset, t.rows[0][1].srcOffset + 2)).toBe("gh");
  });

  it("an empty padded cell falls back to the row line start offset", () => {
    const src = "| a | b |\n| --- | --- |\n| only |";
    const t = parseTable(src, resolve)!;
    // The padded second cell has no source text; its offset must still be a valid position in src.
    expect(t.rows[0][1].srcOffset).toBeGreaterThanOrEqual(0);
    expect(t.rows[0][1].srcOffset).toBeLessThanOrEqual(src.length);
  });
});

describe("parseTable — cell segments (no-regression fidelity)", () => {
  const seg1 = (md: string): CellSegment[] =>
    parseTable(`| ${md} |\n| --- |\n| x |`, resolve)!.header[0].segments;

  it("styles **bold** and hides its markers", () => {
    const segs = seg1("**bold**");
    expect(segs).toEqual([{ kind: "text", text: "bold", classes: ["cm-lp-strong"] }]);
  });

  it("styles `code`, *em*, ~~strike~~", () => {
    expect(seg1("`c`")).toEqual([{ kind: "text", text: "c", classes: ["cm-lp-code"] }]);
    expect(seg1("*e*")).toEqual([{ kind: "text", text: "e", classes: ["cm-lp-em"] }]);
    expect(seg1("~~s~~")).toEqual([{ kind: "text", text: "s", classes: ["cm-lp-strike"] }]);
  });

  it("merges classes for nested emphasis (**a *b***)", () => {
    const segs = seg1("**a *b***");
    expect(segs[0]).toEqual({ kind: "text", text: "a ", classes: ["cm-lp-strong"] });
    expect(segs[1].kind).toBe("text");
    expect((segs[1] as { classes: string[] }).classes).toEqual(
      expect.arrayContaining(["cm-lp-strong", "cm-lp-em"]),
    );
  });

  it("styles [text](url) as a link and hides the url (same rule as the line pass)", () => {
    const segs = seg1("[text](https://x.dev)");
    expect(cellText(segs)).toBe("text");
    expect(segs.some((s) => s.kind === "text" && s.classes.includes("cm-lp-link"))).toBe(true);
  });

  it("leaves block-level syntax literal (# is not a heading inside a table cell)", () => {
    expect(cellText(seg1("# x"))).toBe("# x");
  });

  it("produces a wiki segment for [[target]] (resolved) and [[nope]] (unresolved)", () => {
    const r = seg1("[[target]]");
    expect(r).toEqual([
      { kind: "wiki", label: "target", target: "target", heading: undefined, resolved: "notes/target.md" },
    ]);
    const u = seg1("[[nope]]");
    expect(u[0]).toMatchObject({ kind: "wiki", resolved: undefined });
  });

  it("produces a math segment for $x^2$ but not for currency ($5 and $10)", () => {
    expect(seg1("$x^2$")).toEqual([{ kind: "math", body: "x^2" }]);
    expect(seg1("$5 and $10").every((s) => s.kind === "text")).toBe(true);
  });

  it("wiki wins over math when they overlap ([[$x$]])", () => {
    const segs = seg1("[[$x$]]");
    expect(segs).toHaveLength(1);
    expect(segs[0].kind).toBe("wiki");
  });

  it("mixes plain text with styled runs", () => {
    const segs = seg1("see **b** end");
    expect(segs).toEqual([
      { kind: "text", text: "see ", classes: [] },
      { kind: "text", text: "b", classes: ["cm-lp-strong"] },
      { kind: "text", text: " end", classes: [] },
    ]);
  });
});

describe("parseTable — key (widget eq identity)", () => {
  it("same source + same resolution -> same key; different resolution -> different key", () => {
    const src = "| [[target]] |\n| --- |\n| x |";
    const a = parseTable(src, resolve)!;
    const b = parseTable(src, resolve)!;
    const c = parseTable(src, () => undefined)!;
    expect(a.key).toBe(b.key);
    expect(a.key).not.toBe(c.key);
  });
});
