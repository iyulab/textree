/*
 * Saved-view model — pure (vitest target, no runes/IPC).
 *
 * A view is a saved, named lens over a folder's frontmatter table (folder = database, .md = row):
 * which rows to keep (filters), which columns to show (projection), and how to sort. Views persist
 * to `.textree/views/<slug>.json` (a regeneratable sidecar — file-is-truth; the .md frontmatter is
 * the data, the view is derived config). The on-disk shape is a fixed contract (a future-locking
 * fork once written), so the schema is pinned HERE rather than drifting in across the UI/store.
 *
 * Ratified top-level contract (HANDOFF §3, ROADMAP §3): `{version, name, folder, columns, sort,
 * filters}` — exactly these six fields. Adding a top-level field (e.g. groupBy) is a schema change
 * that needs approval (constitution §4.2), not a silent extension. The INTERNAL shape of each field
 * is free to design.
 *
 * Boundary contract (same as folderTable.helpers): this layer consumes an already-built FolderTable,
 * not raw files — the view is not coupled to how notes are stored or scanned.
 */

import { sortRows, type FolderTable, type FolderTableRow, type SortDir } from "./folderTable.helpers";

/** On-disk schema version. Bump only on a breaking shape change (with a migration). */
export const VIEW_VERSION = 1;

/**
 * Filter operators (MVP — string fields only). `equals` is exact and case-sensitive; `contains` is
 * a case-insensitive substring; `exists`/`missing` test frontmatter-key presence regardless of value.
 * Boolean AND/OR, type-aware comparison, and a query language are intentionally out of scope (YAGNI).
 */
export type FilterOp = "equals" | "contains" | "exists" | "missing";

export interface FilterCondition {
  /** Frontmatter key to test. */
  field: string;
  op: FilterOp;
  /** Compared value (ignored for `exists`/`missing`). */
  value: string;
}

export interface ViewSort {
  /** Frontmatter key, or `null` for the intrinsic Name column. */
  key: string | null;
  dir: SortDir;
}

export interface ViewDefinition {
  version: number;
  /** Display name (also the basis for the on-disk slug). */
  name: string;
  /** Folder path this view belongs to (views are loaded per folder). */
  folder: string;
  /**
   * Explicit column projection. `null` = dynamic union (the read-only default: auto-absorb any
   * frontmatter key). A non-null list is a fixed projection and does NOT auto-absorb new keys —
   * the trade-off is a view can go stale relative to the data, which is the intended "stable view".
   */
  columns: string[] | null;
  /** Persisted sort, or `null` for natural tree order. (Ad-hoc table sort stays ephemeral.) */
  sort: ViewSort | null;
  /** Filter conditions, conjoined (implicit AND). Empty = no filtering. */
  filters: FilterCondition[];
}

/** True when the row satisfies every condition (conjunction). Empty conditions → true. */
export function matchesFilters(row: FolderTableRow, filters: FilterCondition[]): boolean {
  return filters.every((f) => {
    const present = Object.prototype.hasOwnProperty.call(row.fields, f.field);
    switch (f.op) {
      case "exists":
        return present;
      case "missing":
        return !present;
      case "equals":
        return (row.fields[f.field] ?? "") === f.value;
      case "contains":
        return (row.fields[f.field] ?? "").toLowerCase().includes(f.value.toLowerCase());
    }
  });
}

/**
 * Apply a view to a table: filter rows → sort (if set) → project columns. Pure — never mutates the
 * input. `columns: null` keeps the table's dynamic union; an explicit list is used verbatim (kept
 * even if no row has the key, for a stable view).
 */
export function applyView(table: FolderTable, def: ViewDefinition): FolderTable {
  let rows: FolderTableRow[] = def.filters.length
    ? table.rows.filter((r) => matchesFilters(r, def.filters))
    : [...table.rows];
  if (def.sort) rows = sortRows(rows, def.sort.key, def.sort.dir);
  const columns = def.columns ?? table.columns;
  return { columns, rows };
}

/**
 * A view's identity within its folder is its trimmed name. (Views persist in a single folder-keyed
 * file — `.textree/views.json`, mirroring `order.json` — so there is no per-view filename/slug.)
 */
const viewId = (name: string): string => name.trim();

/** Upsert a view into a folder's list, replacing any existing view with the same (trimmed) name. */
export function upsertView(list: ViewDefinition[], view: ViewDefinition): ViewDefinition[] {
  return [...list.filter((v) => viewId(v.name) !== viewId(view.name)), view];
}

/** Remove a view by (trimmed) name from a folder's list. Returns a new array. */
export function removeView(list: ViewDefinition[], name: string): ViewDefinition[] {
  return list.filter((v) => viewId(v.name) !== viewId(name));
}
