import { expect, test } from "vitest";
import { mergeOrder, dedupePushFront, moveInArray, findFirstOpenableNote } from "./nav.helpers";
import type { TreeNode } from "./ipc";

const byPath = (p: string) => p;

/** Leaf node fixture (a .md file — always openable). */
const leaf = (path: string): TreeNode => ({
  name: path,
  kind: "leaf",
  path,
  body_path: path,
  children: [],
});
/** Container fixture; body=path makes it a folder-note (openable), body=null a bare folder. */
const folder = (path: string, children: TreeNode[], body: string | null = null): TreeNode => ({
  name: path,
  kind: "container",
  path,
  body_path: body,
  children,
});

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

test("findFirstOpenableNote: returns the first leaf at the top level", () => {
  const nodes = [leaf("a.md"), leaf("b.md")];
  expect(findFirstOpenableNote(nodes, "", {})?.path).toBe("a.md");
});

test("findFirstOpenableNote: respects nav.order at the root", () => {
  const nodes = [leaf("a.md"), leaf("b.md")];
  expect(findFirstOpenableNote(nodes, "/v", { "/v": ["b.md", "a.md"] })?.path).toBe("b.md");
});

test("findFirstOpenableNote: descends into a bare folder to its first note", () => {
  const nodes = [folder("/v/dir", [leaf("/v/dir/c.md")]), leaf("/v/b.md")];
  expect(findFirstOpenableNote(nodes, "/v", {})?.path).toBe("/v/dir/c.md");
});

test("findFirstOpenableNote: a folder-note container is openable directly", () => {
  const nodes = [folder("/v/dir", [leaf("/v/dir/x.md")], "/v/dir/dir.md")];
  expect(findFirstOpenableNote(nodes, "/v", {})?.path).toBe("/v/dir");
});

test("findFirstOpenableNote: skips an empty bare folder and returns the next note", () => {
  const nodes = [folder("/v/empty", []), leaf("/v/b.md")];
  expect(findFirstOpenableNote(nodes, "/v", {})?.path).toBe("/v/b.md");
});

test("findFirstOpenableNote: returns null when nothing is openable", () => {
  const nodes = [folder("/v/empty", []), folder("/v/empty2", [folder("/v/empty2/deep", [])])];
  expect(findFirstOpenableNote(nodes, "/v", {})).toBeNull();
});

test("findFirstOpenableNote: respects per-subfolder order during recursion", () => {
  const nodes = [folder("/v/dir", [leaf("/v/dir/a.md"), leaf("/v/dir/b.md")])];
  const order = { "/v/dir": ["/v/dir/b.md", "/v/dir/a.md"] };
  expect(findFirstOpenableNote(nodes, "/v", order)?.path).toBe("/v/dir/b.md");
});
