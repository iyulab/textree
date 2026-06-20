import { describe, it, expect } from "vitest";
import { excludeSelf } from "./relatedNotes.helpers";

describe("excludeSelf", () => {
  it("removes the current note and maps fields", () => {
    const hits = [
      { path: "a.md", snippet: "", score: 0.9 },
      { path: "self.md", snippet: "", score: 1.0 },
    ];
    const out = excludeSelf(hits, "self.md");
    expect(out).toEqual([{ path: "a.md", score: 0.9 }]);
  });
  it("is case-insensitive on path", () => {
    const hits = [{ path: "Self.MD", snippet: "", score: 1 }];
    expect(excludeSelf(hits, "self.md")).toEqual([]);
  });
});
