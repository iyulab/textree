import { describe, it, expect } from "vitest";
import { extractFirstH1, isUnnamed, sanitizeForFilename } from "./h1sync.helpers";

describe("extractFirstH1", () => {
  it("returns the first level-1 ATX heading text", () => {
    expect(extractFirstH1("# Meeting notes\n\nbody")).toBe("Meeting notes");
  });
  it("skips frontmatter before the heading", () => {
    expect(extractFirstH1("---\ntitle: x\n---\n# 회의록\n")).toBe("회의록");
  });
  it("ignores headings inside fenced code blocks", () => {
    expect(extractFirstH1("```\n# not a title\n```\n# Real title\n")).toBe("Real title");
  });
  it("ignores level-2+ headings", () => {
    expect(extractFirstH1("## Section\ntext\n")).toBeNull();
  });
  it("requires a space after the hash (not a tag)", () => {
    expect(extractFirstH1("#hashtag\n")).toBeNull();
  });
  it("returns null for an empty heading", () => {
    expect(extractFirstH1("# \nbody")).toBeNull();
  });
  it("strips a closing hash sequence but keeps inline hashes", () => {
    expect(extractFirstH1("# Title #\n")).toBe("Title");
    expect(extractFirstH1("# C#\n")).toBe("C#");
  });
  it("allows up to 3 leading spaces", () => {
    expect(extractFirstH1("   # Indented\n")).toBe("Indented");
  });
  it("returns null when there is no heading", () => {
    expect(extractFirstH1("just text\nmore\n")).toBeNull();
  });
});

describe("isUnnamed", () => {
  it("matches the default and auto-numbered Untitled names", () => {
    expect(isUnnamed("Untitled")).toBe(true);
    expect(isUnnamed("Untitled (1)")).toBe(true);
    expect(isUnnamed("Untitled (12)")).toBe(true);
    expect(isUnnamed("Untitled.md")).toBe(true);
  });
  it("rejects real names", () => {
    expect(isUnnamed("Meeting")).toBe(false);
    expect(isUnnamed("Untitled note")).toBe(false);
    expect(isUnnamed("My Untitled")).toBe(false);
  });
});

describe("sanitizeForFilename", () => {
  it("replaces filesystem-reserved characters with spaces", () => {
    expect(sanitizeForFilename("Plan: A/B")).toBe("Plan A B");
  });
  it("collapses whitespace and trims", () => {
    expect(sanitizeForFilename("  a   b  ")).toBe("a b");
  });
  it("strips leading dots and trailing dots/spaces", () => {
    expect(sanitizeForFilename(".hidden")).toBe("hidden");
    expect(sanitizeForFilename("Notes...")).toBe("Notes");
  });
  it("returns empty string when nothing usable remains", () => {
    expect(sanitizeForFilename("///")).toBe("");
    expect(sanitizeForFilename("   ")).toBe("");
  });
});
