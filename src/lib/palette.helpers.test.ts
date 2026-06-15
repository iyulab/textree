import { describe, it, expect } from "vitest";
import { paletteMode, paletteTerm } from "./palette.helpers";

describe("paletteMode", () => {
  it("'>' prefix is command", () => expect(paletteMode(">새 노트")).toBe("command"));
  it("'/' prefix is content", () => expect(paletteMode("/전문검색")).toBe("content"));
  it("otherwise file", () => expect(paletteMode("노트")).toBe("file"));
  it("empty string is file", () => expect(paletteMode("")).toBe("file"));
});

describe("paletteTerm", () => {
  it("command strips '>' + left trim", () => expect(paletteTerm(">  열기")).toBe("열기"));
  it("content strips '/' + left trim", () => expect(paletteTerm("/  검색")).toBe("검색"));
  it("file keeps the original", () => expect(paletteTerm("노트 ")).toBe("노트 "));
});
