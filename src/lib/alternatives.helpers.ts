/**
 * Pure helpers for alternatives: comparing two versions of a note word by word.
 *
 * A note keeps a paragraph on one line, so comparing line by line would mark a whole paragraph
 * changed for one word. Words are found with the platform's word segmentation, which knows where
 * words end in scripts that do not separate them with spaces; spaces and punctuation are tokens of
 * their own, so a comparison never glues two words together.
 */

/** A run of text that is in both versions, or only in one. */
export type Segment = { kind: "same" | "removed" | "added"; text: string };

let segmenter: Intl.Segmenter | null | undefined;

/** Splits text into words, spaces and punctuation, in order; joined they give the text back. */
export function tokens(text: string): string[] {
  if (segmenter === undefined) {
    segmenter = typeof Intl !== "undefined" && "Segmenter" in Intl ? new Intl.Segmenter(undefined, { granularity: "word" }) : null;
  }
  if (segmenter) return Array.from(segmenter.segment(text), (s) => s.segment);
  return text.match(/[\p{L}\p{N}_]+|\s+|[^\p{L}\p{N}_\s]/gu) ?? [];
}

/**
 * Past this many edits, two versions are shown as one replaced by the other: the search grows with
 * the square of the edits, and a comparison that different says nothing word by word anyway.
 */
export const MAX_EDITS = 4000;

/**
 * The shortest edit from `a` to `b` (Myers), as the tokens of `a` kept or removed and the tokens
 * of `b` added, in reading order.
 */
function editScript(a: string[], b: string[]): Segment[] {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  const offset = max;
  const v = new Int32Array(2 * max + 2);
  // Round d reads only diagonals -d..d of the round before, so that window is all that is kept.
  const trace: Int32Array[] = [];
  let found = -1;
  for (let d = 0; d <= max && found < 0; d++) {
    if (d > MAX_EDITS) {
      return [
        { kind: "removed", text: a.join("") },
        { kind: "added", text: b.join("") },
      ];
    }
    trace.push(v.slice(offset - d, offset + d + 1));
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1]) ? v[offset + k + 1] : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        found = d;
        break;
      }
    }
  }
  // Walk back from the end through the recorded frontiers.
  const out: Segment[] = [];
  let x = n;
  let y = m;
  for (let d = found; d > 0; d--) {
    const prev = trace[d];
    const at = (diagonal: number) => prev[diagonal + d];
    const k = x - y;
    const down = k === -d || (k !== d && at(k - 1) < at(k + 1));
    const prevK = down ? k + 1 : k - 1;
    const prevX = at(prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      out.push({ kind: "same", text: a[--x] });
      y--;
    }
    if (down) out.push({ kind: "added", text: b[--y] });
    else out.push({ kind: "removed", text: a[--x] });
  }
  while (x > 0 && y > 0) {
    out.push({ kind: "same", text: a[--x] });
    y--;
  }
  return out.reverse();
}

/** Adjacent segments of one kind joined into one. */
function joined(segments: Segment[]): Segment[] {
  const out: Segment[] = [];
  for (const s of segments) {
    if (s.text === "") continue;
    const last = out[out.length - 1];
    if (last && last.kind === s.kind) last.text += s.text;
    else out.push({ ...s });
  }
  return out;
}

/**
 * How `after` differs from `before`, word by word. Joined, the `same` and `removed` segments give
 * `before` back and the `same` and `added` ones give `after`.
 */
export function wordDiff(before: string, after: string): Segment[] {
  const a = tokens(before);
  const b = tokens(after);
  // What both start and end with needs no search, and in two versions of one note that is most of it.
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const head: Segment[] = a.slice(0, start).map((text) => ({ kind: "same", text }));
  const tail: Segment[] = a.slice(endA).map((text) => ({ kind: "same", text }));
  const middle = editScript(a.slice(start, endA), b.slice(start, endB));
  return joined([...head, ...middle, ...tail]);
}

/** The two sides of a side-by-side comparison: what each version holds, with its own changes marked. */
export function sides(segments: Segment[]): { before: Segment[]; after: Segment[] } {
  return {
    before: segments.filter((s) => s.kind !== "added"),
    after: segments.filter((s) => s.kind !== "removed"),
  };
}

/** Whether two versions differ in anything but whitespace at their ends. */
export function differ(before: string, after: string): boolean {
  return before.trim() !== after.trim();
}
