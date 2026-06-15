import { expect, test } from "vitest";
import { mergeOrder, dedupePushFront, moveInArray } from "./nav.helpers";

const byPath = (p: string) => p;

test("order 지정 항목이 먼저, 지정 순서대로", () => {
  const sibs = ["a.md", "b.md", "c.md"];
  const order = ["c.md", "a.md"];
  expect(mergeOrder(sibs, order, byPath)).toEqual(["c.md", "a.md", "b.md"]);
});

test("order 없는 항목은 원래 순서로 뒤에", () => {
  const sibs = ["a.md", "b.md", "c.md"];
  expect(mergeOrder(sibs, [], byPath)).toEqual(["a.md", "b.md", "c.md"]);
});

test("order에 있으나 실재하지 않는 경로는 무시(자기치유)", () => {
  const sibs = ["a.md", "b.md"];
  const order = ["zzz.md", "b.md"];
  expect(mergeOrder(sibs, order, byPath)).toEqual(["b.md", "a.md"]);
});

test("dedupePushFront: 맨 앞 삽입·중복 제거·상한", () => {
  expect(dedupePushFront(["b", "c"], "a", 3)).toEqual(["a", "b", "c"]);
  expect(dedupePushFront(["b", "a", "c"], "a", 3)).toEqual(["a", "b", "c"]);
  expect(dedupePushFront(["b", "c", "d"], "a", 3)).toEqual(["a", "b", "c"]);
});

test("moveInArray: 위/아래 이동과 경계 보존", () => {
  expect(moveInArray(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
  expect(moveInArray(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
  expect(moveInArray(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]); // 맨 위에서 위로 = 무변화
  expect(moveInArray(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);  // 맨 아래에서 아래로 = 무변화
});
