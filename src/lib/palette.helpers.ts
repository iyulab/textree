/*
 * Palette mode/search-term derivation — pure logic (runes-independent, vitest target).
 * '>' = command, '/' = content search, otherwise = filename fuzzy.
 */

export type PaletteMode = "file" | "command" | "content";

export function paletteMode(query: string): PaletteMode {
  if (query.startsWith(">")) return "command";
  if (query.startsWith("/")) return "content";
  return "file";
}

export function paletteTerm(query: string): string {
  const mode = paletteMode(query);
  if (mode === "file") return query;
  return query.slice(1).trimStart();
}

/**
 * Status of the results list, so the palette can distinguish an in-flight search from a
 * settled "no matches" (both render zero rows otherwise). Drives a status row in the UI.
 * - results:    at least one row to show (kept even mid-search to avoid flicker)
 * - searching:  content query is in flight and nothing is on screen yet
 * - no-results: the user typed a query but it matched nothing
 * - empty:      no query yet and nothing listed (idle prompt)
 */
export type PaletteListState = "results" | "searching" | "no-results" | "empty";

export function paletteListState(args: {
  mode: PaletteMode;
  term: string;
  count: number;
  searching: boolean;
}): PaletteListState {
  if (args.count > 0) return "results";
  if (args.mode === "content" && args.searching) return "searching";
  if (args.term !== "") return "no-results";
  return "empty";
}
