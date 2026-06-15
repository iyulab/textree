/*
 * Pure-function fuzzy matcher — subsequence matching + weighted scoring.
 * No external dependencies (minimal). Score weights: consecutive matches, word-boundary start, earlier position.
 */

export interface Match<T> {
  item: T;
  score: number;
  /** Matched character position ranges (for highlighting), [start, end) half-open. */
  ranges: [number, number][];
}

function isBoundary(text: string, i: number): boolean {
  if (i === 0) return true;
  const prev = text[i - 1];
  const cur = text[i];
  // After a separator, or a lowercase→uppercase transition (camelCase boundary).
  return (
    prev === "/" ||
    prev === "-" ||
    prev === "_" ||
    prev === " " ||
    prev === "." ||
    (prev === prev.toLowerCase() && cur === cur.toUpperCase() && cur !== cur.toLowerCase())
  );
}

/** Score for a single candidate string. null if matching fails. */
function scoreOne(query: string, text: string): { score: number; ranges: [number, number][] } | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let score = 0;
  const positions: number[] = [];
  let ti = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    let found = -1;
    for (let k = ti; k < t.length; k++) {
      if (t[k] === ch) {
        found = k;
        break;
      }
    }
    if (found === -1) return null; // subsequence not satisfied
    // Weighting: add for earlier position (smaller found), add at a boundary, add if consecutive with the previous match.
    // The position bonus approximates a 20-char leading window. Beyond that (tail of long names), boundary/consecutive
    // weights drive the ranking (YAGNI: tune relative weights if measured dissatisfaction arises).
    score += Math.max(0, 20 - found);
    if (isBoundary(text, found)) score += 15;
    if (positions.length && found === positions[positions.length - 1] + 1) score += 10;
    positions.push(found);
    ti = found + 1;
  }
  // Compress consecutive positions into [start,end) ranges.
  const ranges: [number, number][] = [];
  for (const p of positions) {
    const last = ranges[ranges.length - 1];
    if (last && last[1] === p) last[1] = p + 1;
    else ranges.push([p, p + 1]);
  }
  return { score, ranges };
}

/**
 * Fuzzy-match the candidate array and return it in descending score order. Non-matching candidates are excluded.
 * On a tie, prefer the shorter candidate string (treated as the more precise match).
 * For an empty query (`""`), return all items in original order with score 0 and an empty ranges array.
 */
export function fuzzyMatch<T>(
  query: string,
  items: T[],
  key: (item: T) => string,
): Match<T>[] {
  if (query === "") return items.map((item) => ({ item, score: 0, ranges: [] }));
  const out: Match<T>[] = [];
  for (const item of items) {
    const text = key(item);
    const s = scoreOne(query, text);
    if (s) out.push({ item, score: s.score, ranges: s.ranges });
  }
  out.sort((a, b) => b.score - a.score || key(a.item).length - key(b.item).length);
  return out;
}
