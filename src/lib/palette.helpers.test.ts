import { describe, it, expect } from "vitest";
import { paletteMode, paletteTerm } from "./palette.helpers";

describe("paletteMode", () => {
  it("'>' 접두는 command", () => expect(paletteMode(">새 노트")).toBe("command"));
  it("'/' 접두는 content", () => expect(paletteMode("/전문검색")).toBe("content"));
  it("그 외는 file", () => expect(paletteMode("노트")).toBe("file"));
  it("빈 문자열은 file", () => expect(paletteMode("")).toBe("file"));
});

describe("paletteTerm", () => {
  it("command는 '>' 제거 + 좌측 trim", () => expect(paletteTerm(">  열기")).toBe("열기"));
  it("content는 '/' 제거 + 좌측 trim", () => expect(paletteTerm("/  검색")).toBe("검색"));
  it("file은 원본 유지", () => expect(paletteTerm("노트 ")).toBe("노트 "));
});
