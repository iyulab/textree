/**
 * Reading a folder's settings files — telling a file that is not there from one that cannot be
 * read, and checking that what parses is the shape it should be.
 *
 * A file that parses into the wrong shape is as unreadable as one that does not parse: taken as it
 * is, a list that is not a list breaks the screen, and a map of the wrong things is mixed with new
 * entries and written back. Either way the settings are neither used nor written over — see
 * `settingsFile.ts`.
 *
 * Pure: no runes, no IPC.
 */

import type { ViewDefinition } from "./view.helpers";

export type ParsedSettings<T> = { kind: "absent" } | { kind: "ok"; value: T } | { kind: "damaged" };

export function parseSettings<T>(
  raw: string | null,
  isShape: (value: unknown) => value is T,
): ParsedSettings<T> {
  if (raw === null) return { kind: "absent" };
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return { kind: "damaged" };
  }
  return isShape(value) ? { kind: "ok", value } : { kind: "damaged" };
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

export const isStringList = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((x) => typeof x === "string");

export const isStringListMap = (v: unknown): v is Record<string, string[]> =>
  isRecord(v) && Object.values(v).every(isStringList);

function isView(v: unknown): v is ViewDefinition {
  if (!isRecord(v)) return false;
  const { version, name, folder, columns, sort, filters } = v;
  return (
    typeof version === "number" &&
    typeof name === "string" &&
    typeof folder === "string" &&
    (columns === null || isStringList(columns)) &&
    (sort === null || (isRecord(sort) && (sort.key === null || typeof sort.key === "string"))) &&
    Array.isArray(filters) &&
    filters.every((f) => isRecord(f) && typeof f.field === "string" && typeof f.op === "string")
  );
}

export const isViewsMap = (v: unknown): v is Record<string, ViewDefinition[]> =>
  isRecord(v) && Object.values(v).every((list) => Array.isArray(list) && list.every(isView));

/** What each settings file holds, in words a person uses. */
export function settingsLabel(rel: string): string {
  switch (rel) {
    case "favorites.json":
      return "favorites";
    case "order.json":
      return "note order";
    case "views.json":
      return "saved views";
    default:
      return rel;
  }
}

/** The line shown when a folder opens, if its settings need one. */
export function describeSettingsOnOpen(readOnly: boolean, setAside: string[]): string | null {
  if (readOnly) {
    return "This folder's settings were saved by a newer Textree. They're kept as they are until you update — changes to favorites, note order and saved views won't be saved here.";
  }
  if (setAside.length === 0) return null;
  const what = setAside.map((name) => settingsLabel(name.replace(/\.unreadable-.*$/, "")));
  const listed = what.length === 1 ? what[0] : `${what.slice(0, -1).join(", ")} and ${what.at(-1)}`;
  const one = setAside.length === 1;
  return `This folder's ${listed} couldn't be read, so ${one ? "it starts" : "they start"} empty. The unreadable ${one ? "file was" : "files were"} kept (${setAside.join(", ")}).`;
}
