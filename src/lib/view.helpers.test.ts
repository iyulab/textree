import { describe, expect, it } from "vitest";
import type { FolderTable, FolderTableRow } from "./folderTable.helpers";
import {
  applyView,
  findForeignViewFolders,
  matchesFilters,
  removeView,
  upsertView,
  VIEW_VERSION,
  type FilterCondition,
  type ViewDefinition,
} from "./view.helpers";

const row = (name: string, fields: Record<string, string>): FolderTableRow => ({
  name,
  path: `/v/${name}.md`,
  fields,
});

const filter = (field: string, op: FilterCondition["op"], value = ""): FilterCondition => ({
  field,
  op,
  value,
});

const view = (over: Partial<ViewDefinition> = {}): ViewDefinition => ({
  version: VIEW_VERSION,
  name: "View",
  folder: "/v",
  columns: null,
  sort: null,
  filters: [],
  ...over,
});

describe("matchesFilters", () => {
  it("matches all rows when there are no conditions", () => {
    expect(matchesFilters(row("a", { status: "done" }), [])).toBe(true);
  });

  it("equals matches an exact field value (case-sensitive)", () => {
    const r = row("a", { status: "done" });
    expect(matchesFilters(r, [filter("status", "equals", "done")])).toBe(true);
    expect(matchesFilters(r, [filter("status", "equals", "Done")])).toBe(false);
    expect(matchesFilters(r, [filter("status", "equals", "wip")])).toBe(false);
  });

  it("equals treats a missing field as empty", () => {
    expect(matchesFilters(row("a", {}), [filter("status", "equals", "")])).toBe(true);
    expect(matchesFilters(row("a", {}), [filter("status", "equals", "done")])).toBe(false);
  });

  it("contains matches a case-insensitive substring", () => {
    const r = row("a", { tags: "Work/Urgent" });
    expect(matchesFilters(r, [filter("tags", "contains", "urgent")])).toBe(true);
    expect(matchesFilters(r, [filter("tags", "contains", "WORK")])).toBe(true);
    expect(matchesFilters(r, [filter("tags", "contains", "home")])).toBe(false);
  });

  it("exists matches when the frontmatter key is present (even if empty)", () => {
    expect(matchesFilters(row("a", { due: "" }), [filter("due", "exists")])).toBe(true);
    expect(matchesFilters(row("a", { status: "x" }), [filter("due", "exists")])).toBe(false);
  });

  it("missing matches when the frontmatter key is absent", () => {
    expect(matchesFilters(row("a", { status: "x" }), [filter("due", "missing")])).toBe(true);
    expect(matchesFilters(row("a", { due: "" }), [filter("due", "missing")])).toBe(false);
  });

  it("conjoins multiple conditions (implicit AND)", () => {
    const r = row("a", { status: "done", prio: "high" });
    expect(
      matchesFilters(r, [filter("status", "equals", "done"), filter("prio", "equals", "high")]),
    ).toBe(true);
    expect(
      matchesFilters(r, [filter("status", "equals", "done"), filter("prio", "equals", "low")]),
    ).toBe(false);
  });
});

describe("applyView", () => {
  const table: FolderTable = {
    columns: ["status", "prio"],
    rows: [
      row("a", { status: "done", prio: "high" }),
      row("b", { status: "wip", prio: "low" }),
      row("c", { status: "done", prio: "low" }),
    ],
  };

  it("returns the table unchanged for a default view (no filter/sort, dynamic columns)", () => {
    const out = applyView(table, view());
    expect(out.columns).toEqual(["status", "prio"]);
    expect(out.rows.map((r) => r.name)).toEqual(["a", "b", "c"]);
  });

  it("filters rows by conditions", () => {
    const out = applyView(table, view({ filters: [filter("status", "equals", "done")] }));
    expect(out.rows.map((r) => r.name)).toEqual(["a", "c"]);
  });

  it("sorts rows when sort is set", () => {
    const out = applyView(table, view({ sort: { key: "prio", dir: "asc" } }));
    expect(out.rows.map((r) => r.name)).toEqual(["a", "b", "c"]); // high < low; lows tie by name
  });

  it("filters then sorts", () => {
    const out = applyView(
      table,
      view({ filters: [filter("status", "equals", "done")], sort: { key: "prio", dir: "desc" } }),
    );
    expect(out.rows.map((r) => r.name)).toEqual(["c", "a"]); // done rows a(high),c(low); desc → low,high
  });

  it("projects explicit columns and does not auto-absorb new keys", () => {
    const out = applyView(table, view({ columns: ["prio"] }));
    expect(out.columns).toEqual(["prio"]);
    expect(out.rows.length).toBe(3); // rows keep all fields; only the column list narrows
  });

  it("keeps an explicit column even if no row has it (stable view over stale data)", () => {
    const out = applyView(table, view({ columns: ["status", "ghost"] }));
    expect(out.columns).toEqual(["status", "ghost"]);
  });

  it("does not mutate the input table", () => {
    applyView(table, view({ filters: [filter("status", "equals", "done")], sort: { key: "prio", dir: "asc" } }));
    expect(table.rows.map((r) => r.name)).toEqual(["a", "b", "c"]);
  });
});

describe("upsertView / removeView", () => {
  const v = (name: string): ViewDefinition => view({ name });

  it("appends a new view", () => {
    expect(upsertView([v("A")], v("B")).map((x) => x.name)).toEqual(["A", "B"]);
  });

  it("replaces a view with the same (trimmed) name, in place of the old", () => {
    const updated = view({ name: "A", filters: [filter("status", "equals", "done")] });
    const out = upsertView([v("A"), v("B")], updated);
    expect(out.map((x) => x.name)).toEqual(["B", "A"]); // old A dropped, updated A appended
    expect(out.find((x) => x.name === "A")?.filters).toHaveLength(1);
  });

  it("treats names differing only by surrounding whitespace as the same view", () => {
    expect(upsertView([v("A")], v("  A  ")).length).toBe(1);
  });

  it("removes a view by trimmed name and does not mutate the input", () => {
    const list = [v("A"), v("B")];
    expect(removeView(list, " A ").map((x) => x.name)).toEqual(["B"]);
    expect(list.map((x) => x.name)).toEqual(["A", "B"]);
  });

  it("removing a missing name is a no-op", () => {
    expect(removeView([v("A")], "Z").map((x) => x.name)).toEqual(["A"]);
  });
});

describe("findForeignViewFolders", () => {
  it("returns nothing when every stored key lives under the current vault root", () => {
    const keys = ["D:/vault/diary", "D:/vault/sub/notes"];
    expect(findForeignViewFolders(keys, "D:/vault")).toEqual([]);
  });

  it("flags keys that belong to a different absolute location (vault moved / other device)", () => {
    const keys = ["C:/old-vault/diary", "D:/vault/diary"];
    expect(findForeignViewFolders(keys, "D:/vault")).toEqual(["C:/old-vault/diary"]);
  });

  it("normalizes mixed path separators before comparing (Windows backslashes)", () => {
    // views.json keys can mix separators (e.g. `D:/vault\diary`); root may use backslashes
    const keys = ["D:/vault\\diary"];
    expect(findForeignViewFolders(keys, "D:\\vault")).toEqual([]);
  });

  it("does not flag the root folder itself", () => {
    expect(findForeignViewFolders(["D:/vault"], "D:/vault")).toEqual([]);
  });

  it("tolerates a trailing slash on the root", () => {
    expect(findForeignViewFolders(["D:/vault/diary"], "D:/vault/")).toEqual([]);
  });

  it("treats a sibling folder with a shared prefix as foreign (not a real descendant)", () => {
    // "D:/vault2" starts with "D:/vault" textually but is a different folder
    expect(findForeignViewFolders(["D:/vault2/diary"], "D:/vault")).toEqual(["D:/vault2/diary"]);
  });

  it("treats drive-letter / path case differences as the same location (Windows is case-insensitive)", () => {
    // root from openVault and keys from selectedNode.path can differ only in case — not foreign
    expect(findForeignViewFolders(["D:/Vault/Diary"], "d:/vault")).toEqual([]);
  });

  it("returns nothing for an empty key set", () => {
    expect(findForeignViewFolders([], "D:/vault")).toEqual([]);
  });
});
