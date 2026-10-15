import { describe, expect, it } from "vitest";
import { absOut, relIn } from "./vaultPathsAdapter";

describe("transitional path adapter", () => {
  it("strips the vault prefix the backend handed out, either separator", () => {
    expect(relIn("D:\\notes", "D:\\notes\\a\\b.md")).toBe("a/b.md");
    expect(relIn("D:/notes", "D:/notes/a/b.md")).toBe("a/b.md");
    expect(relIn("D:\\notes\\", "D:\\notes\\b.md")).toBe("b.md");
  });

  it("matches the vault prefix the way Windows compares paths", () => {
    expect(relIn("d:\\Notes", "D:\\notes\\b.md")).toBe("b.md");
  });

  it("refuses a path outside the vault, or the vault itself, instead of guessing", () => {
    expect(() => relIn("D:\\notes", "D:\\other\\b.md")).toThrow();
    expect(() => relIn("D:\\notes", "D:\\notes2\\b.md")).toThrow();
    expect(() => relIn("D:\\notes", "D:\\notes")).toThrow();
  });

  it("joins back with the separator the vault path uses", () => {
    expect(absOut("D:\\notes", "a/b.md")).toBe("D:\\notes\\a\\b.md");
    expect(absOut("D:/notes", "a/b.md")).toBe("D:/notes/a/b.md");
    expect(absOut("D:\\notes\\", "b.md")).toBe("D:\\notes\\b.md");
  });
});
