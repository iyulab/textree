/*
 * Frontmatter table model — pure (vitest target, no runes/IPC).
 *
 * The "folder = database, .md = row" view (ROADMAP §2 basics, table stakes). This helper turns a
 * folder's notes — each already parsed into scalar frontmatter — into a table model: the union of
 * frontmatter keys as columns, one row per note.
 *
 * Boundary contract (constitution / ROADMAP §2): this layer consumes ALREADY-PARSED frontmatter,
 * not raw files or a storage handle. The view is therefore not coupled to how notes are stored or
 * scanned — an in-memory model today (D17: in-memory first, no SQLite) that a future indexed
 * backend could feed without changing this contract.
 */

export interface FolderTableRow {
  /** Note display name (the intrinsic first column, rendered by the component). */
  name: string;
  /** Note path — the address to open the row. */
  path: string;
  /** Parsed scalar frontmatter fields for this note. */
  fields: Record<string, string>;
}

export interface FolderTable {
  /** Frontmatter field keys, unioned across rows, ordered by first appearance (deterministic). */
  columns: string[];
  /** One row per input note, in input order. */
  rows: FolderTableRow[];
}

export interface FolderTableNote {
  name: string;
  path: string;
  frontmatter: Record<string, string>;
}

export type SortDir = "asc" | "desc";

/**
 * Sort rows by a column. `key === null` sorts by the intrinsic Name column; otherwise by the
 * frontmatter field `key` (a missing field compares as empty). Numeric-looking values order
 * naturally (2 before 10). Ties break by name for a stable, deterministic order. Pure — returns a
 * new array; the sort is ephemeral UI state (no persisted view definition, so no format to fix yet).
 */
export function sortRows(rows: FolderTableRow[], key: string | null, dir: SortDir): FolderTableRow[] {
  const value = (r: FolderTableRow) => (key === null ? r.name : (r.fields[key] ?? ""));
  const sign = dir === "desc" ? -1 : 1;
  return [...rows].sort((a, b) => {
    const primary = value(a).localeCompare(value(b), undefined, { numeric: true });
    if (primary !== 0) return sign * primary;
    return a.name.localeCompare(b.name, undefined, { numeric: true }); // stable tie-break (always asc)
  });
}

/** Build the table model from a folder's notes (already parsed into frontmatter). */
export function buildFolderTable(notes: FolderTableNote[]): FolderTable {
  const columns: string[] = [];
  const seen = new Set<string>();
  const rows: FolderTableRow[] = [];
  for (const n of notes) {
    for (const key of Object.keys(n.frontmatter)) {
      if (!seen.has(key)) {
        seen.add(key);
        columns.push(key);
      }
    }
    rows.push({ name: n.name, path: n.path, fields: n.frontmatter });
  }
  return { columns, rows };
}
