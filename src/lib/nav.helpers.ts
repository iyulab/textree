/*
 * Navigation pure helpers — runes-free pure functions/constants.
 * Separated from nav.svelte.ts (the runes store) to test safely under vitest (node environment).
 */

/** Upper bound for the recent list. */
export const RECENT_MAX = 20;

/**
 * Merge-sorts sibling nodes according to the order array (pure function).
 * Places items listed in order at the front in the given order, and the rest behind in original order.
 * Paths present in order but absent from siblings are ignored (self-healing for deleted paths).
 */
export function mergeOrder<T>(
  siblings: T[],
  order: string[],
  key: (item: T) => string,
): T[] {
  const bySib = new Map(siblings.map((s) => [key(s), s]));
  const placed = new Set<string>();
  const head: T[] = [];
  for (const k of order) {
    const item = bySib.get(k);
    if (item !== undefined && !placed.has(k)) {
      head.push(item);
      placed.add(k);
    }
  }
  const tail = siblings.filter((s) => !placed.has(key(s)));
  return [...head, ...tail];
}

/** Puts value at the front, removes duplicates, then truncates to max items (pure). */
export function dedupePushFront(list: string[], value: string, max: number): string[] {
  return [value, ...list.filter((v) => v !== value)].slice(0, max);
}

/**
 * Returns a new array with the item at index moved by delta positions (+1 = down, -1 = up) (pure).
 * If out of range (up from the top, down from the bottom), returns the original unchanged.
 */
export function moveInArray<T>(arr: T[], index: number, delta: number): T[] {
  const to = index + delta;
  if (index < 0 || index >= arr.length || to < 0 || to >= arr.length) return arr;
  const next = arr.slice();
  const [item] = next.splice(index, 1);
  next.splice(to, 0, item);
  return next;
}
