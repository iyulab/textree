/*
 * 팔레트 모드/검색어 파생 — 순수 로직(runes 비의존, vitest 대상).
 * '>'=명령, '/'=본문검색, 그 외=파일명 fuzzy.
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
