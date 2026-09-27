import { describe, expect, it } from "vitest";
import { differ, MAX_EDITS, noticeFor, sides, tokens, wordDiff, type Segment } from "./alternatives.helpers";

const text = (segments: Segment[]) => segments.map((s) => s.text).join("");
const kinds = (segments: Segment[]) => segments.map((s) => `${s.kind}:${s.text}`);

/** Both versions come back out of the comparison unchanged. */
function roundTrips(before: string, after: string): void {
  const { before: left, after: right } = sides(wordDiff(before, after));
  expect(text(left)).toBe(before);
  expect(text(right)).toBe(after);
}

describe("tokens", () => {
  it("gives the text back when joined", () => {
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    for (const t of ["", "Hello, world!", "회의록 초안입니다.\n\n다음 주 월요일", "a  b\tc", "日本語の文章"]) {
      expect(tokens(t).join("")).toBe(t);
    }
  });

  it("keeps words, spaces and punctuation apart", () => {
    expect(tokens("one, two")).toEqual(["one", ",", " ", "two"]);
  });
});

describe("wordDiff", () => {
  it("marks only the word that changed in a long paragraph", () => {
    const before = "The meeting is on Monday at the usual place.";
    const after = "The meeting is on Tuesday at the usual place.";
    expect(kinds(wordDiff(before, after))).toEqual([
      "same:The meeting is on ",
      "removed:Monday",
      "added:Tuesday",
      "same: at the usual place.",
    ]);
  });

  it("finds word boundaries in Korean", () => {
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    const segments = wordDiff("다음 회의는 월요일에 합니다.", "다음 회의는 화요일에 합니다.");
    const changed = segments.filter((s) => s.kind !== "same").map((s) => s.text);
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    expect(changed.join("")).not.toContain("다음");
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    expect(changed.join("")).not.toContain("합니다");
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    roundTrips("다음 회의는 월요일에 합니다.", "다음 회의는 화요일에 합니다.");
  });

  it("shows a moved paragraph as gone from one place and added at the other", () => {
    const before = "First.\n\nSecond.\n\nThird.\n";
    const after = "Second.\n\nThird.\n\nFirst.\n";
    const segments = wordDiff(before, after);
    expect(segments.some((s) => s.kind === "removed" && s.text.includes("First"))).toBe(true);
    expect(segments.some((s) => s.kind === "added" && s.text.includes("First"))).toBe(true);
    roundTrips(before, after);
  });

  it("says nothing changed for identical versions, and handles empty ones", () => {
    expect(kinds(wordDiff("same text", "same text"))).toEqual(["same:same text"]);
    expect(wordDiff("", "")).toEqual([]);
    expect(kinds(wordDiff("", "new"))).toEqual(["added:new"]);
    expect(kinds(wordDiff("old", ""))).toEqual(["removed:old"]);
  });

  it("round-trips any pair of versions", () => {
    // forbidden-tokens: allow non-latin - word boundaries in scripts other than Latin are what this case is about
    const words = ["alpha", "beta", "가나", "다라", " ", "\n", ",", "."];
    let seed = 7;
    const pick = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return words[seed % words.length];
    };
    for (let i = 0; i < 200; i++) {
      const before = Array.from({ length: seed % 40 }, pick).join("");
      const after = Array.from({ length: (seed >> 3) % 40 }, pick).join("");
      roundTrips(before, after);
    }
  });

  it("shows versions too different to compare word by word as one replaced by the other", () => {
    const before = Array.from({ length: MAX_EDITS + 10 }, (_, i) => `a${i}`).join(" ");
    const after = Array.from({ length: MAX_EDITS + 10 }, (_, i) => `b${i}`).join(" ");
    const segments = wordDiff(before, after);
    expect(segments.map((s) => s.kind)).toEqual(["removed", "added"]);
    roundTrips(before, after);
  });
});

describe("differ", () => {
  it("ignores whitespace at the ends only", () => {
    expect(differ("text\n", "text")).toBe(false);
    expect(differ("text", "text!")).toBe(true);
  });
});

describe("noticeFor", () => {
  const alt = (over: Partial<{ id: string; rel: string; arrived: boolean; differs: boolean }>) => ({
    id: "1",
    rel: "a.md",
    arrived: false,
    differs: false,
    ...over,
  });

  it("says nothing without an open note or an alternative of it", () => {
    expect(noticeFor([alt({})], null)).toBeNull();
    expect(noticeFor([alt({ rel: "b.md" })], "a.md")).toBeNull();
  });

  it("shows one started here even before it differs", () => {
    expect(noticeFor([alt({})], "a.md")?.text).toBe("You have an alternative of this note.");
  });

  it("hides one that arrived holding what the note holds", () => {
    expect(noticeFor([alt({ arrived: true, differs: false })], "a.md")).toBeNull();
  });

  it("puts one from elsewhere first", () => {
    const notice = noticeFor([alt({ id: "mine" }), alt({ id: "theirs", arrived: true, differs: true })], "a.md");
    expect(notice?.alternative.id).toBe("theirs");
    expect(notice?.text).toBe("This note also changed elsewhere.");
  });
});
