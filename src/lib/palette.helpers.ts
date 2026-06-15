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
