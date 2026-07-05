/*
 * Callout header parsing — pure (no DOM/CM imports; vitest targets this file).
 *
 * A blockquote whose first line opens with `[!type]` is a callout. Five core
 * styles exist; aliases and unknown types fall back so nothing breaks, and
 * the displayed title respects the author's typed word. Fold suffixes
 * (`-`/`+`) are accepted and ignored (no fold behavior in v1 — fold state is
 * app state, deferred). Recognition parity with the published renderer is
 * pinned in callout-parity.golden.json (byte-identical copy of canopy's).
 */

export type CalloutCore = "note" | "tip" | "warning" | "danger" | "quote";

export const CALLOUT_CORES: readonly CalloutCore[] = ["note", "tip", "warning", "danger", "quote"];

const CALLOUT_ALIASES: Record<string, CalloutCore> = {
  note: "note", info: "note", abstract: "note", summary: "note", tldr: "note", todo: "note", example: "note",
  tip: "tip", hint: "tip", important: "tip",
  warning: "warning", caution: "warning", attention: "warning", question: "warning", help: "warning", faq: "warning",
  danger: "danger", error: "danger", failure: "danger", fail: "danger", missing: "danger", bug: "danger",
  quote: "quote", cite: "quote",
};

/**
 * Callout icon path data (24x24 viewBox, 2px stroke) — byte-identical copy of
 * canopy's CALLOUT_ICON_PATHS (src/styles.ts) so the editor and the published
 * page draw the same glyphs; update both together.
 */
export const CALLOUT_ICON_PATHS: Record<CalloutCore, string> = {
  note: "M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
  tip: "M9 18h6M10 22h4M15.09 14c.18-.98.65-1.74 1.41-2.5A4.65 4.65 0 0 0 18 8 6 6 0 0 0 6 8c0 1 .23 2.23 1.5 3.5A4.61 4.61 0 0 1 8.91 14",
  warning: "m21.73 18-8-14a2 2 0 0 0-3.46 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3M12 9v4M12 17h.01",
  danger: "M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86L7.86 2zM12 8v4M12 16h.01",
  quote: "M10 11H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v6a4 4 0 0 1-4 4M20 11h-4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v6a4 4 0 0 1-4 4",
};

export interface CalloutHeader {
  core: CalloutCore;
  rawType: string;
  /** Explicit text after the marker, or the typed word with its first letter uppercased. */
  title: string;
  /** True when the author wrote a title on the marker line. */
  explicitTitle: boolean;
  /** Length of the `[!type]` marker including an optional fold suffix (`-`/`+`). */
  markerLength: number;
}

// `[!type]` opening the line: ASCII letters only, optional fold suffix, then
// either the end of the line or whitespace and a title. Junk right after the
// marker (e.g. `[!note]x`) means this is not a callout.
const HEADER_RE = /^\[!([A-Za-z]+)\]([-+])?(?:[ \t]+(.*))?$/;

/** Parse a quote's first content line (after the `> ` marker); null when not a callout header. */
export function parseCalloutHeader(text: string): CalloutHeader | null {
  const m = HEADER_RE.exec(text);
  if (m === null) return null;
  const rawType = m[1] as string;
  const fold = m[2];
  const explicit = (m[3] ?? "").trim();
  return {
    core: CALLOUT_ALIASES[rawType.toLowerCase()] ?? "note",
    rawType,
    title: explicit !== "" ? explicit : rawType.charAt(0).toUpperCase() + rawType.slice(1),
    explicitTitle: explicit !== "",
    markerLength: rawType.length + 3 + (fold === undefined ? 0 : 1),
  };
}
