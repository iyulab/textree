/**
 * Why an action on the open note did not happen, when its edits were not all saved first.
 *
 * `failed`: the save itself failed — the banner on the note says more. `busy`: nothing failed, the
 * edits were still arriving (a key held down) — trying again in a moment works. Telling the two
 * apart matters: "could not save" for the second would send someone looking for a problem that
 * is not there.
 */
export function leaveRefusal(action: string, left: "failed" | "busy"): string {
  return left === "busy"
    ? `${action} canceled — your last edits are still being saved. Try again in a moment.`
    : `${action} canceled — could not save your unsaved edits.`;
}
