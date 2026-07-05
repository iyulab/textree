/*
 * KaTeX rendering adapter for the live-preview editor.
 *
 * Same library and version as the published site (canopy), so rendered math is byte-identical.
 * `throwOnError: false` matches rehype-katex's default: invalid LaTeX renders as an error node
 * (red) rather than throwing, so a mid-typing formula never crashes the editor.
 */
import katex from "katex";

/** Render LaTeX source to an HTML string. `displayMode` = centered block ($$) vs inline ($). */
export function renderMath(body: string, displayMode: boolean): string {
  return katex.renderToString(body, { displayMode, throwOnError: false });
}
