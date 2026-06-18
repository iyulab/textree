import { describe, expect, it } from "vitest";
import { buildFolderTable, sortRows, type FolderTableRow } from "./folderTable.helpers";

const note = (name: string, frontmatter: Record<string, string>) => ({
  name,
  path: `/v/${name}.md`,
  frontmatter,
});

const row = (name: string, fields: Record<string, string>): FolderTableRow => ({
  name,
  path: `/v/${name}.md`,
  fields,
});

describe("buildFolderTable", () => {
  it("collects columns as the union of frontmatter keys, ordered by first appearance", () => {
    const table = buildFolderTable([
      note("a", { status: "done", tags: "x" }),
      note("b", { status: "wip", due: "2026-06-18" }),
    ]);
    expect(table.columns).toEqual(["status", "tags", "due"]);
  });

  it("keeps one row per note, preserving input order and field values", () => {
    const table = buildFolderTable([
      note("first", { status: "done" }),
      note("second", { status: "wip" }),
    ]);
    expect(table.rows).toEqual([
      { name: "first", path: "/v/first.md", fields: { status: "done" } },
      { name: "second", path: "/v/second.md", fields: { status: "wip" } },
    ]);
  });

  it("lists a note with no frontmatter as a row that contributes no columns", () => {
    const table = buildFolderTable([note("plain", {}), note("tagged", { tag: "a" })]);
    expect(table.columns).toEqual(["tag"]);
    expect(table.rows.map((r) => r.name)).toEqual(["plain", "tagged"]);
    expect(table.rows[0].fields).toEqual({});
  });

  it("does not duplicate a column shared by multiple notes", () => {
    const table = buildFolderTable([
      note("a", { status: "done" }),
      note("b", { status: "wip" }),
      note("c", { status: "done" }),
    ]);
    expect(table.columns).toEqual(["status"]);
  });

  it("returns an empty table for no notes", () => {
    expect(buildFolderTable([])).toEqual({ columns: [], rows: [] });
  });
});

describe("sortRows", () => {
  it("sorts by a frontmatter field ascending", () => {
    const rows = [row("a", { status: "wip" }), row("b", { status: "done" })];
    expect(sortRows(rows, "status", "asc").map((r) => r.name)).toEqual(["b", "a"]);
  });

  it("sorts by a frontmatter field descending", () => {
    const rows = [row("a", { status: "done" }), row("b", { status: "wip" })];
    expect(sortRows(rows, "status", "desc").map((r) => r.name)).toEqual(["b", "a"]);
  });

  it("sorts by name when key is null", () => {
    const rows = [row("beta", {}), row("alpha", {})];
    expect(sortRows(rows, null, "asc").map((r) => r.name)).toEqual(["alpha", "beta"]);
  });

  it("treats a missing field as empty (sorts first ascending)", () => {
    const rows = [row("a", { due: "2026" }), row("b", {})];
    expect(sortRows(rows, "due", "asc").map((r) => r.name)).toEqual(["b", "a"]);
  });

  it("orders numeric-looking values naturally (2 before 10)", () => {
    const rows = [row("a", { n: "10" }), row("b", { n: "2" })];
    expect(sortRows(rows, "n", "asc").map((r) => r.name)).toEqual(["b", "a"]);
  });

  it("breaks ties by name and does not mutate the input", () => {
    const rows = [row("z", { s: "x" }), row("a", { s: "x" })];
    const sorted = sortRows(rows, "s", "asc");
    expect(sorted.map((r) => r.name)).toEqual(["a", "z"]);
    expect(rows.map((r) => r.name)).toEqual(["z", "a"]); // original untouched
  });
});
