import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { parseCalloutHeader } from "./callout.helpers";

/*
 * Golden guard for the `editor` column of callout-parity.golden.json — the
 * shared spec pinning how the editor's callout recognition and canopy's
 * published renderer see the same source (the copy here must stay
 * byte-identical to canopy's `src/callout-parity.golden.json`; bump
 * `version` and update both together). Cases tagged `editorGate` are
 * rejected by a non-scanner gate in livePreview (e.g. the Lezer code-fence
 * exclusion), which the pure parser cannot see; their editor assertion is
 * skipped.
 */
interface GoldenCallout {
  type: string;
  title: string;
}

interface GoldenCase {
  name: string;
  markdown: string;
  published: GoldenCallout[];
  editor: GoldenCallout[];
  note?: string;
  editorGate?: string;
}

const spec = JSON.parse(
  readFileSync(new URL("./callout-parity.golden.json", import.meta.url), "utf8"),
) as { version: number; cases: GoldenCase[] };

/**
 * Ordered callouts the editor recognizes, per the livePreview wiring: only a
 * top-level quote's first line carries a header (a `>` line whose previous
 * line is not part of the same quote), matching the Blockquote branch.
 */
function classifyEditor(markdown: string): GoldenCallout[] {
  const out: GoldenCallout[] = [];
  let prevIsQuote = false;
  for (const line of markdown.split("\n")) {
    const m = /^ {0,3}> ?(.*)$/.exec(line);
    if (m !== null && !prevIsQuote) {
      const h = parseCalloutHeader(m[1] as string);
      if (h !== null) out.push({ type: h.core, title: h.title });
    }
    prevIsQuote = m !== null;
  }
  return out;
}

describe("callout parity golden — editor column", () => {
  for (const c of spec.cases) {
    if (c.editorGate !== undefined) continue;
    it(`${c.name}`, () => {
      expect(classifyEditor(c.markdown)).toEqual(c.editor);
    });
  }
});
