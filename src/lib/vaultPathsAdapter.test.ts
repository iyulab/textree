import { describe, expect, it } from "vitest";
import { absOut, folderRelIn, relIn } from "./vaultPathsAdapter";

const bs = "\\";

describe("transitional path adapter", () => {
  it("strips the vault prefix the backend handed out, either separator", () => {
    expect(relIn(`D:${bs}notes`, `D:${bs}notes${bs}a${bs}b.md`)).toBe("a/b.md");
    expect(relIn("D:/notes", "D:/notes/a/b.md")).toBe("a/b.md");
    expect(relIn(`D:${bs}notes${bs}`, `D:${bs}notes${bs}b.md`)).toBe("b.md");
  });

  it("matches the vault prefix the way Windows compares paths", () => {
    expect(relIn(`d:${bs}Notes`, `D:${bs}notes${bs}b.md`)).toBe("b.md");
  });

  it("refuses a path outside the vault, or the vault itself, instead of guessing", () => {
    expect(() => relIn(`D:${bs}notes`, `D:${bs}other${bs}b.md`)).toThrow();
    expect(() => relIn(`D:${bs}notes`, `D:${bs}notes2${bs}b.md`)).toThrow();
    expect(() => relIn(`D:${bs}notes`, `D:${bs}notes`)).toThrow();
  });

  it("names the vault itself as an empty folder path", () => {
    expect(folderRelIn("D:/notes", "d:/Notes/")).toBe("");
    expect(folderRelIn("D:/notes", "D:/notes/sub")).toBe("sub");
  });

  it("spells a path the way the backend joins one on Windows", () => {
    expect(absOut(`D:${bs}notes`, "a/b.md")).toBe(`D:${bs}notes${bs}a${bs}b.md`);
    // The vault path is kept as given; what follows it is joined with a backslash.
    expect(absOut("D:/notes", "a/b.md")).toBe(`D:/notes${bs}a${bs}b.md`);
    expect(absOut(`D:${bs}notes${bs}`, "b.md")).toBe(`D:${bs}notes${bs}b.md`);
    expect(absOut("D:/notes/", "b.md")).toBe("D:/notes/b.md");
  });
});
