import { expect, test } from "vitest";
import {
  buildBacklinkIndex,
  buildWikiResolver,
  findHeadingOffset,
  findWikiLinks,
  parseWikiTarget,
  wikiCompletions,
  wikiLabel,
  wikiRenderSpans,
} from "./wikilink.helpers";

// ── parseWikiTarget ──────────────────────────────────────────────────────

test("parses a bare target", () => {
  expect(parseWikiTarget("idea")).toEqual({ target: "idea", heading: undefined, alias: undefined });
});

test("splits alias after the pipe", () => {
  expect(parseWikiTarget("note|Shown")).toEqual({
    target: "note",
    heading: undefined,
    alias: "Shown",
  });
});

test("splits heading after the hash", () => {
  expect(parseWikiTarget("note#Section")).toEqual({
    target: "note",
    heading: "Section",
    alias: undefined,
  });
});

test("splits pipe before hash so [[a#b|c]] yields all three", () => {
  expect(parseWikiTarget("a#b|c")).toEqual({ target: "a", heading: "b", alias: "c" });
});

test("trims whitespace in every component", () => {
  expect(parseWikiTarget("  note  #  Section  |  Alias  ")).toEqual({
    target: "note",
    heading: "Section",
    alias: "Alias",
  });
});

test("empty target keeps a heading (same-document link)", () => {
  expect(parseWikiTarget("#Section")).toEqual({
    target: "",
    heading: "Section",
    alias: undefined,
  });
});

test("blank components collapse to undefined", () => {
  expect(parseWikiTarget("note#|")).toEqual({ target: "note", heading: undefined, alias: undefined });
});

test("block anchor stays as a heading fragment (no special handling yet)", () => {
  expect(parseWikiTarget("note#^abc123")).toEqual({
    target: "note",
    heading: "^abc123",
    alias: undefined,
  });
});

// ── wikiLabel ────────────────────────────────────────────────────────────

test("label prefers the alias", () => {
  expect(wikiLabel({ target: "note", heading: "h", alias: "Shown" })).toBe("Shown");
});

test("label falls back to the target", () => {
  expect(wikiLabel({ target: "note", heading: "h", alias: undefined })).toBe("note");
});

test("label of a same-document heading link is #heading", () => {
  expect(wikiLabel({ target: "", heading: "Section", alias: undefined })).toBe("#Section");
});

// ── findWikiLinks ────────────────────────────────────────────────────────

test("finds a single link with its span", () => {
  const [span] = findWikiLinks("see [[idea]] here");
  expect(span).toMatchObject({ target: "idea", from: 4, to: 12, embed: false, label: "idea" });
  expect("see [[idea]] here".slice(span.from, span.to)).toBe("[[idea]]");
});

test("finds multiple links in order", () => {
  const spans = findWikiLinks("[[a]] and [[b|B]]");
  expect(spans.map((s) => s.target)).toEqual(["a", "b"]);
  expect(spans[1].label).toBe("B");
});

test("detects an embed prefix and includes the ! in the span", () => {
  const [span] = findWikiLinks("![[diagram]]");
  expect(span).toMatchObject({ target: "diagram", embed: true, from: 0, to: 12 });
  expect("![[diagram]]".slice(span.from, span.to)).toBe("![[diagram]]");
});

test("skips a blank link", () => {
  expect(findWikiLinks("empty [[]] and [[  ]]")).toEqual([]);
});

test("does not run a link across brackets or newlines", () => {
  // The inner class forbids `[`/`]`/newline, so a stray bracket cannot extend the match.
  expect(findWikiLinks("[[a]\n[b]]")).toEqual([]);
});

test("is stateful-regex safe across repeated calls", () => {
  // Guards against a leaked lastIndex on the module-level regex.
  expect(findWikiLinks("[[x]]")).toHaveLength(1);
  expect(findWikiLinks("[[x]]")).toHaveLength(1);
});

// ── buildWikiResolver ────────────────────────────────────────────────────

test("resolves a bare name by stem, case-insensitively", () => {
  const r = buildWikiResolver(["notes/Idea.md"]);
  expect(r.resolve("idea")).toBe("notes/Idea.md");
  expect(r.resolve("IDEA")).toBe("notes/Idea.md");
});

test("resolves a path-bearing target by full path", () => {
  const r = buildWikiResolver(["a/idea.md", "b/idea.md"]);
  expect(r.resolve("b/idea")).toBe("b/idea.md");
});

test("a .md suffix on the target is ignored", () => {
  const r = buildWikiResolver(["notes/idea.md"]);
  expect(r.resolve("idea.md")).toBe("notes/idea.md");
});

test("name collisions resolve deterministically — shortest path then lexicographic", () => {
  const r = buildWikiResolver(["z/deep/idea.md", "a/idea.md", "b/idea.md"]);
  // Same stem; shortest path wins (a/ and b/ are depth 2, z/deep/ depth 3); a < b lexicographically.
  expect(r.resolve("idea")).toBe("a/idea.md");
});

test("resolution is independent of input order", () => {
  const forward = buildWikiResolver(["a/idea.md", "b/idea.md"]).resolve("idea");
  const reverse = buildWikiResolver(["b/idea.md", "a/idea.md"]).resolve("idea");
  expect(forward).toBe(reverse);
  expect(forward).toBe("a/idea.md");
});

test("unresolved and empty targets return undefined", () => {
  const r = buildWikiResolver(["notes/idea.md"]);
  expect(r.resolve("missing")).toBeUndefined();
  expect(r.resolve("")).toBeUndefined();
});

test("backslash paths normalize to POSIX for matching", () => {
  const r = buildWikiResolver(["notes/idea.md"]);
  expect(r.resolve("notes\\idea")).toBe("notes/idea.md");
});

// ── wikiRenderSpans ──────────────────────────────────────────────────────

const resolveIdea = (t: string) => (t.toLowerCase() === "idea" ? "notes/idea.md" : undefined);
const never = () => false;

test("renders a resolved link with its path", () => {
  const [span] = wikiRenderSpans("see [[idea]]", resolveIdea, never);
  expect(span).toMatchObject({ label: "idea", resolved: "notes/idea.md", embed: false });
});

test("marks an unresolved target with resolved=undefined", () => {
  const [span] = wikiRenderSpans("[[missing]]", resolveIdea, never);
  expect(span.resolved).toBeUndefined();
});

test("a same-document heading link resolves to the empty string (link-styled, no note)", () => {
  const [span] = wikiRenderSpans("[[#Top]]", resolveIdea, never);
  expect(span).toMatchObject({ target: "", heading: "Top", resolved: "" });
});

test("excluded spans are dropped (caller skips code / cursor line / frontmatter)", () => {
  const doc = "`[[idea]]` and [[idea]]";
  // Exclude the first occurrence only (positions 0..10 cover the inline-code span).
  const spans = wikiRenderSpans(doc, resolveIdea, (from) => from < 11);
  expect(spans).toHaveLength(1);
  expect(spans[0].from).toBe(doc.indexOf("[[idea]]", 5));
});

test("carries the alias label and heading through", () => {
  const [span] = wikiRenderSpans("[[idea#Plan|Roadmap]]", resolveIdea, never);
  expect(span).toMatchObject({ label: "Roadmap", heading: "Plan", resolved: "notes/idea.md" });
});

// ── buildBacklinkIndex ───────────────────────────────────────────────────

// Stem-name resolver over a fixed note set (mirrors buildWikiResolver semantics).
const notesFixture = [
  { path: "a.md", body: "links to [[idea]] and [[idea|again]]" },
  { path: "b.md", body: "see [[Idea]] (case-insensitive)" },
  { path: "notes/idea.md", body: "self ref [[idea]] should not count, but [[a]] does" },
  { path: "c.md", body: "in `[[idea]]` code — excluded from backlinks" },
];
const fixtureResolve = buildWikiResolver(notesFixture.map((n) => n.path)).resolve;

test("collects sources that link to a note, deduped by source", () => {
  const idx = buildBacklinkIndex(notesFixture, fixtureResolve);
  const back = idx.to("notes/idea.md");
  // a.md links twice (deduped → once), b.md once; idea.md self-link and c.md code link excluded.
  expect(back.map((b) => b.from)).toEqual(["a.md", "b.md"]);
});

test("links inside code (fenced or inline) are not backlinks", () => {
  const idx = buildBacklinkIndex(
    [
      { path: "doc.md", body: "real [[idea]]\n```\ncode [[idea]] fenced\n```\ninline `[[idea]]`" },
      { path: "notes/idea.md", body: "" },
    ],
    buildWikiResolver(["doc.md", "notes/idea.md"]).resolve,
  );
  // Only the one real link counts → doc.md appears once.
  expect(idx.to("notes/idea.md").map((b) => b.from)).toEqual(["doc.md"]);
});

test("keeps the first label for a deduped source", () => {
  const idx = buildBacklinkIndex(notesFixture, fixtureResolve);
  const fromA = idx.to("notes/idea.md").find((b) => b.from === "a.md");
  expect(fromA?.label).toBe("idea");
});

test("excludes self-links", () => {
  const idx = buildBacklinkIndex(notesFixture, fixtureResolve);
  expect(idx.to("notes/idea.md").some((b) => b.from === "notes/idea.md")).toBe(false);
});

test("records resolved links across folders", () => {
  const idx = buildBacklinkIndex(notesFixture, fixtureResolve);
  // notes/idea.md links to [[a]] → a.md gets a backlink from notes/idea.md.
  expect(idx.to("a.md").map((b) => b.from)).toEqual(["notes/idea.md"]);
});

test("unresolved targets and same-document links produce no backlinks", () => {
  const idx = buildBacklinkIndex(
    [{ path: "x.md", body: "[[missing]] and [[#heading]]" }],
    fixtureResolve,
  );
  expect(idx.to("x.md")).toEqual([]);
});

test("a note with no backlinks returns an empty array", () => {
  const idx = buildBacklinkIndex(notesFixture, fixtureResolve);
  expect(idx.to("b.md")).toEqual([]);
});

test("backlinks are sorted by source path and order-independent", () => {
  const forward = buildBacklinkIndex(notesFixture, fixtureResolve).to("notes/idea.md");
  const reversed = buildBacklinkIndex([...notesFixture].reverse(), fixtureResolve).to(
    "notes/idea.md",
  );
  expect(forward).toEqual(reversed);
});

// ── wikiCompletions ──────────────────────────────────────────────────────

const completionPaths = ["Inbox.md", "notes/Idea.md", "archive/Idea.md", "Journal.md"];

test("filters by query against the stem, case-insensitively", () => {
  const opts = wikiCompletions(completionPaths, "idea");
  expect(opts.map((o) => o.label)).toEqual(["Idea", "Idea"]);
});

test("unique stem inserts just the stem (editor adds the closing brackets)", () => {
  const [opt] = wikiCompletions(completionPaths, "inbox");
  expect(opt).toEqual({ label: "Inbox", detail: undefined, insert: "Inbox" });
});

test("ambiguous stem inserts the full path and shows it as detail", () => {
  const opts = wikiCompletions(completionPaths, "idea");
  expect(opts.every((o) => o.detail !== undefined)).toBe(true);
  expect(opts.map((o) => o.insert).sort()).toEqual(["archive/Idea", "notes/Idea"]);
});

test("an empty query returns every note (capped at 50)", () => {
  expect(wikiCompletions(completionPaths, "").map((o) => o.label).sort()).toEqual([
    "Idea",
    "Idea",
    "Inbox",
    "Journal",
  ]);
  expect(wikiCompletions(Array.from({ length: 80 }, (_, i) => `n${i}.md`), "")).toHaveLength(50);
});

test("prefix matches rank before substring matches", () => {
  // "al" prefixes Alpha but only appears mid-word in Journal → Alpha ranks first.
  const opts = wikiCompletions(["Journal.md", "Alpha.md"], "al");
  expect(opts.map((o) => o.label)).toEqual(["Alpha", "Journal"]);
});

test("matches against the path, not only the stem", () => {
  expect(wikiCompletions(completionPaths, "archive").map((o) => o.label)).toEqual(["Idea"]);
});

// ── findHeadingOffset ────────────────────────────────────────────────────

const headingDoc = "# Title\n\nintro\n\n## Plan\n\nbody\n\n### Deep Dive\n";

test("finds a heading by text, case-insensitively, returning its line offset", () => {
  const off = findHeadingOffset(headingDoc, "plan");
  expect(off).toBe(headingDoc.indexOf("## Plan"));
});

test("matches a deeper heading regardless of level", () => {
  expect(findHeadingOffset(headingDoc, "Deep Dive")).toBe(headingDoc.indexOf("### Deep Dive"));
});

test("returns null when the heading is absent or empty", () => {
  expect(findHeadingOffset(headingDoc, "Nope")).toBeNull();
  expect(findHeadingOffset(headingDoc, "")).toBeNull();
});

test("a # in body text is not a heading", () => {
  expect(findHeadingOffset("text with #Plan inline\n", "Plan")).toBeNull();
});

test("a ^block-anchor fragment matches its line", () => {
  const doc = "first line\nanchored paragraph ^abc123\nlast\n";
  expect(findHeadingOffset(doc, "^abc123")).toBe(doc.indexOf("anchored"));
});
