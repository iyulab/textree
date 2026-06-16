/*
 * Wikilink `[[` autocomplete — suggests vault notes while typing inside `[[`.
 *
 * Candidate selection and ranking are pure (wikilink.helpers `wikiCompletions`, vitest-tested); this
 * module is the thin CodeMirror adapter that maps a `[[query` match to CodeMirror completions. Added
 * to the editor via the same compartment as the resolver, so suggestions track the live vault tree.
 */

import {
  autocompletion,
  completionKeymap,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { Prec } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { wikiCompletions } from "./wikilink.helpers";

/** Build the `[[` autocomplete extension over the given vault note paths. */
export function wikiAutocomplete(notePaths: readonly string[]) {
  const source = (context: CompletionContext): CompletionResult | null => {
    // Match `[[` followed by the partial query — no nested/closing bracket, no newline.
    const before = context.matchBefore(/\[\[[^[\]\n]*/);
    if (before === null) return null;
    const query = before.text.slice(2); // drop the leading `[[`
    // Stop suggesting once a heading (#) or alias (|) begins — those are typed freely.
    if (query.includes("#") || query.includes("|")) return null;
    const options = wikiCompletions(notePaths, query);
    if (options.length === 0 && !context.explicit) return null;
    // Supply the closing `]]` unless the cursor already sits before one (editing an existing link),
    // which would otherwise duplicate it into `[[note]]]]`.
    const closing = context.state.sliceDoc(context.pos, context.pos + 2) === "]]" ? "" : "]]";
    return {
      from: before.from + 2, // replace only the query, keeping the `[[`
      options: options.map(
        (o): Completion => ({
          label: o.label,
          detail: o.detail,
          apply: o.insert + closing,
          type: "text",
        }),
      ),
    };
  };

  // The completion keymap is raised above the editor's default keymap so Enter/Tab accept an open
  // suggestion instead of inserting a newline. When no completion is active, its bindings fall
  // through to the default behavior. (Default keymap disabled here to avoid binding it twice.)
  return [
    autocompletion({ override: [source], defaultKeymap: false }),
    Prec.high(keymap.of(completionKeymap)),
  ];
}
