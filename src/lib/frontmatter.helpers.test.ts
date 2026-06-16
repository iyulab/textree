import { expect, test } from "vitest";
import { parseFrontmatter, getField } from "./frontmatter.helpers";

test("no frontmatter: whole document is the body", () => {
  const fm = parseFrontmatter("# Title\n\nbody text");
  expect(fm.present).toBe(false);
  expect(fm.data).toEqual({});
  expect(fm.body).toBe("# Title\n\nbody text");
  expect(fm.end).toBe(0);
});

test("parses scalar fields and strips the block from the body", () => {
  const doc = "---\ntitle: Hello\nicon: 📓\n---\n# Heading\nbody";
  const fm = parseFrontmatter(doc);
  expect(fm.present).toBe(true);
  expect(fm.data).toEqual({ title: "Hello", icon: "📓" });
  expect(fm.body).toBe("# Heading\nbody");
});

test("empty frontmatter block is recognized with no fields", () => {
  const fm = parseFrontmatter("---\n---\nbody");
  expect(fm.present).toBe(true);
  expect(fm.data).toEqual({});
  expect(fm.body).toBe("body");
});

test("unterminated block is left in the body (never swallowed)", () => {
  const doc = "---\ntitle: Hello\nno closing fence";
  const fm = parseFrontmatter(doc);
  expect(fm.present).toBe(false);
  expect(fm.body).toBe(doc);
  expect(fm.end).toBe(0);
});

test("a lone --- (horizontal rule) is not frontmatter", () => {
  const fm = parseFrontmatter("---\n");
  expect(fm.present).toBe(false);
});

test("strips matching surrounding quotes from values", () => {
  const fm = parseFrontmatter("---\ntitle: \"Quoted Title\"\ncover: 'img.png'\n---\nx");
  expect(fm.data).toEqual({ title: "Quoted Title", cover: "img.png" });
});

test("ignores comments and blank lines inside the block", () => {
  const fm = parseFrontmatter("---\n# a comment\n\ntitle: Kept\n---\nx");
  expect(fm.data).toEqual({ title: "Kept" });
});

test("values may contain colons (only the first colon splits)", () => {
  const fm = parseFrontmatter("---\ncover: https://example.com/a.png\n---\nx");
  expect(fm.data.cover).toBe("https://example.com/a.png");
});

test("handles CRLF line endings", () => {
  const fm = parseFrontmatter("---\r\ntitle: Win\r\n---\r\nbody");
  expect(fm.present).toBe(true);
  expect(fm.data).toEqual({ title: "Win" });
  expect(fm.body).toBe("body");
});

test("last value wins on duplicate keys", () => {
  const fm = parseFrontmatter("---\ntitle: First\ntitle: Second\n---\nx");
  expect(fm.data.title).toBe("Second");
});

test("frontmatter is only recognized at the document start", () => {
  const fm = parseFrontmatter("intro\n---\ntitle: Late\n---\nx");
  expect(fm.present).toBe(false);
});

test("body + stripped prefix reconstructs the original document", () => {
  const doc = "---\ntitle: T\n---\nline1\nline2";
  const fm = parseFrontmatter(doc);
  expect(doc.slice(0, fm.end) + fm.body).toBe(doc);
});

test("getField is case-insensitive and prefers an exact match", () => {
  expect(getField({ Title: "X" }, "title")).toBe("X");
  expect(getField({ title: "lower", Title: "upper" }, "title")).toBe("lower");
  expect(getField({}, "missing")).toBeUndefined();
});
