import { describe, it, expect } from "vitest";
import { paletteMode, paletteTerm, paletteListState } from "./palette.helpers";

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

describe("paletteListState", () => {
  it("reports results whenever there is at least one match", () => {
    expect(paletteListState({ mode: "file", term: "no", count: 3, searching: false })).toBe(
      "results",
    );
  });

  it("keeps showing results during an in-flight content search (no flicker to empty)", () => {
    expect(paletteListState({ mode: "content", term: "abc", count: 2, searching: true })).toBe(
      "results",
    );
  });

  it("reports searching for a content query in flight with no results yet", () => {
    expect(paletteListState({ mode: "content", term: "abc", count: 0, searching: true })).toBe(
      "searching",
    );
  });

  it("reports no-results for a settled content search that found nothing", () => {
    expect(paletteListState({ mode: "content", term: "abc", count: 0, searching: false })).toBe(
      "no-results",
    );
  });

  it("reports no-results when a file query matches nothing", () => {
    expect(paletteListState({ mode: "file", term: "zzz", count: 0, searching: false })).toBe(
      "no-results",
    );
  });

  it("reports no-results when a command query matches nothing", () => {
    expect(paletteListState({ mode: "command", term: "zzz", count: 0, searching: false })).toBe(
      "no-results",
    );
  });

  it("reports empty (not no-results) when the query is blank and nothing is listed", () => {
    expect(paletteListState({ mode: "file", term: "", count: 0, searching: false })).toBe("empty");
  });

  it("reports empty for a blank content query (idle, before typing)", () => {
    expect(paletteListState({ mode: "content", term: "", count: 0, searching: false })).toBe(
      "empty",
    );
  });
});
