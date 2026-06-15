import { expect, test } from "vitest";
import { fuzzyMatch } from "./fuzzy";

test("매칭되지 않는 후보는 제외된다", () => {
  const r = fuzzyMatch("xyz", ["apple", "banana"], (s) => s);
  expect(r).toHaveLength(0);
});

test("subsequence 매칭 + 대소문자 무시", () => {
  const r = fuzzyMatch("ap", ["Apple", "grape", "xyz"], (s) => s);
  expect(r.map((m) => m.item)).toEqual(["Apple", "grape"]);
});

test("연속·경계 매치가 더 높은 점수", () => {
  const r = fuzzyMatch("fb", ["afubar", "foo-bar"], (s) => s);
  expect(r[0].item).toBe("foo-bar"); // 경계 매치 우선
});

test("하이라이트 범위를 반환한다", () => {
  const r = fuzzyMatch("ab", ["aXb"], (s) => s);
  expect(r[0].ranges).toEqual([
    [0, 1],
    [2, 3],
  ]);
});

test("연속 매치는 단일 range로 압축된다", () => {
  const r = fuzzyMatch("ab", ["abX"], (s) => s);
  expect(r[0].ranges).toEqual([[0, 2]]);
});

test("한글 후보 매칭", () => {
  const r = fuzzyMatch("회의", ["회의록", "일정표"], (s) => s);
  expect(r.map((m) => m.item)).toEqual(["회의록"]);
});
