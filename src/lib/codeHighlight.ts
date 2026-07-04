/*
 * Fenced code-block syntax highlighting for the live-preview editor.
 *
 * Two pieces wire into Editor.svelte:
 *  - `codeLanguages` feeds @codemirror/lang-markdown's `codeLanguages` option so a fence's
 *    language (```js) nests that grammar (lazily imported on first use).
 *  - `codeHighlighting` maps the resulting @lezer/highlight tags to the app's --syntax-* tokens.
 *
 * Parity note: the editor uses CodeMirror (lezer) highlighting; the published site uses Shiki
 * (canopy). The two engines never produce byte-identical output — the goal is "both highlight",
 * not "identical". Code fences are standard markdown, so the .md round-trips regardless (D21).
 */
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

/** Lazy-loaded language descriptors for markdown's `codeLanguages` fence option. */
export const codeLanguages = languages;

/**
 * Maps lezer highlight tags to the app's --syntax-* design tokens. Colors are CSS vars, so
 * light/dark resolve automatically via the same [data-theme] switch as every other token.
 */
const syntaxTokens = HighlightStyle.define([
  { tag: [t.keyword, t.controlKeyword, t.moduleKeyword, t.operatorKeyword], color: "var(--syntax-keyword)" },
  { tag: [t.string, t.special(t.string), t.regexp, t.escape], color: "var(--syntax-string)" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: [t.number, t.bool, t.atom], color: "var(--syntax-number)" },
  {
    tag: [t.function(t.variableName), t.function(t.definition(t.variableName)), t.propertyName, t.definition(t.variableName)],
    color: "var(--syntax-name)",
  },
  { tag: [t.typeName, t.className, t.namespace, t.tagName], color: "var(--syntax-type)" },
  { tag: [t.punctuation, t.bracket, t.operator, t.derefOperator], color: "var(--syntax-punctuation)" },
]);

/** Extension bundle: applies the syntax highlight style to nested code languages. */
export const codeHighlighting: Extension = syntaxHighlighting(syntaxTokens);
