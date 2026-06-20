import type { SemanticHit } from "./ipc";

export interface RelatedNote {
  path: string;
  score: number;
}

/**
 * Filter out the current note from a semantic-search result and map to `RelatedNote`.
 * Path comparison is case-insensitive to tolerate Windows file-system casing variations.
 */
export function excludeSelf(hits: SemanticHit[], selfPath: string): RelatedNote[] {
  const self = selfPath.toLowerCase();
  return hits
    .filter((h) => h.path.toLowerCase() !== self)
    .map((h) => ({ path: h.path, score: h.score }));
}
