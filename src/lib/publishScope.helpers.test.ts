import { describe, expect, it } from "vitest";
import cases from "./publishScope.cases.json";
import { isIncluded, relativeTo, type PublishChoice } from "./publishScope.helpers";

describe("isIncluded", () => {
  for (const c of cases as { why: string; choices: PublishChoice[]; rel: string; included: boolean }[]) {
    it(c.why, () => {
      expect(isIncluded(c.rel, c.choices)).toBe(c.included);
    });
  }
});

describe("relativeTo", () => {
  it("turns backslash paths into relative posix paths", () => {
    expect(relativeTo("C:\\Notes", "C:\\Notes\\a\\b.md")).toBe("a/b.md");
  });
  it("ignores case in the prefix and keeps the case of the rest", () => {
    expect(relativeTo("C:\\Notes\\", "c:\\NOTES\\Sub\\X.md")).toBe("Sub/X.md");
  });
  it("is empty for the folder itself", () => {
    expect(relativeTo("C:\\Notes", "c:/notes")).toBe("");
  });
  it("is null outside the folder, including a sibling with the same prefix", () => {
    expect(relativeTo("C:\\Notes", "C:\\Other\\a.md")).toBeNull();
    expect(relativeTo("C:\\Notes", "C:\\Notes2\\a.md")).toBeNull();
  });
});

