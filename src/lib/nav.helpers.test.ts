import { expect, test } from "vitest";
import { mergeOrder, dedupePushFront, moveInArray } from "./nav.helpers";

const byPath = (p: string) => p;

test("ordered items come first, in the given order", () => {
  const sibs = ["a.md", "b.md", "c.md"];
  const order = ["c.md", "a.md"];
  expect(mergeOrder(sibs, order, byPath)).toEqual(["c.md", "a.md", "b.md"]);
});

test("items without an order entry follow in original order", () => {
  const sibs = ["a.md", "b.md", "c.md"];
  expect(mergeOrder(sibs, [], byPath)).toEqual(["a.md", "b.md", "c.md"]);
});

test("paths in order but not present are ignored (self-healing)", () => {
  const sibs = ["a.md", "b.md"];
  const order = ["zzz.md", "b.md"];
  expect(mergeOrder(sibs, order, byPath)).toEqual(["b.md", "a.md"]);
});

test("dedupePushFront: insert at front, dedupe, cap", () => {
  expect(dedupePushFront(["b", "c"], "a", 3)).toEqual(["a", "b", "c"]);
  expect(dedupePushFront(["b", "a", "c"], "a", 3)).toEqual(["a", "b", "c"]);
  expect(dedupePushFront(["b", "c", "d"], "a", 3)).toEqual(["a", "b", "c"]);
});

test("moveInArray: move up/down and preserve bounds", () => {
  expect(moveInArray(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
  expect(moveInArray(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
  expect(moveInArray(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]); // up from the top = no change
  expect(moveInArray(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);  // down from the bottom = no change
});
