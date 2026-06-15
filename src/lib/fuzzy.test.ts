import { expect, test } from "vitest";
import { fuzzyMatch } from "./fuzzy";

test("non-matching candidates are excluded", () => {
  const r = fuzzyMatch("xyz", ["apple", "banana"], (s) => s);
  expect(r).toHaveLength(0);
});

test("subsequence match + case-insensitive", () => {
  const r = fuzzyMatch("ap", ["Apple", "grape", "xyz"], (s) => s);
  expect(r.map((m) => m.item)).toEqual(["Apple", "grape"]);
});

test("consecutive and boundary matches score higher", () => {
  const r = fuzzyMatch("fb", ["afubar", "foo-bar"], (s) => s);
  expect(r[0].item).toBe("foo-bar"); // boundary match preferred
});

test("returns highlight ranges", () => {
  const r = fuzzyMatch("ab", ["aXb"], (s) => s);
  expect(r[0].ranges).toEqual([
    [0, 1],
    [2, 3],
  ]);
});

test("consecutive matches collapse into a single range", () => {
  const r = fuzzyMatch("ab", ["abX"], (s) => s);
  expect(r[0].ranges).toEqual([[0, 2]]);
});

test("Korean candidate matching", () => {
  const r = fuzzyMatch("회의", ["회의록", "일정표"], (s) => s);
  expect(r.map((m) => m.item)).toEqual(["회의록"]);
});
