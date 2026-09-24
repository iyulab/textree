// Pure decision logic for the Sync Coordinator (sync.ts) — vitest imports this directly.

export type RescanAction = "none" | "reload" | "conflict";

/**
 * Reconcile the open note after a watcher rescan (the OS dropped events, or another tool moved the
 * repository's checked-out branch, so the note MAY have changed externally with no per-path event).
 * A rescan carries no evidence about this note, so the decision is made from content alone, as a
 * three-way comparison between what is on disk, what the user sees (`live`), and the last content
 * the editor and disk were known to agree on (`synced` — last loaded or last saved):
 *
 * - disk matches what the user sees → nothing to do (no reload, no cursor/focus disturbance);
 * - differs while clean → silent reload (FS is the truth);
 * - differs while dirty, but disk is still exactly what was last synced → the only difference is
 *   the user's own unsaved typing; nothing changed on disk, so nothing to ask about;
 * - differs while dirty and disk moved away from the last sync → a real external change meets
 *   unsaved edits → conflict banner.
 */
export function decideRescanAction(disk: string, live: string, dirty: boolean, synced: string): RescanAction {
  if (disk === live) return "none";
  if (!dirty) return "reload";
  return disk === synced ? "none" : "conflict";
}

export type ChangeAction = "reload" | "conflict" | "none";

/**
 * Decide what a per-path change to the open note means. When disk already holds what the user
 * sees, there is nothing to take in — an event about the app's own move or rename of the open note,
 * or a touch that changed no content, must not rebuild the editor under the cursor. Otherwise a
 * clean note reloads (FS is the truth). A dirty note only has a conflict when disk no longer holds
 * what was last synced — a change event whose content is still that (a late echo) is not an
 * external edit, and raising a banner for it would ask the user about their own typing.
 */
export function decideChangeAction(disk: string, live: string, dirty: boolean, synced: string): ChangeAction {
  if (disk === live) return "none";
  if (!dirty) return "reload";
  return disk === synced ? "none" : "conflict";
}

export type ReopenAction = "keep" | "rebuild" | "reuse";

/**
 * The note already open was opened again (picked in the tree, or a version of it restored). Its
 * editor keeps its key, so without a rebuild the screen goes on showing what it showed.
 *
 * - its own edits are still being written → `keep`: the screen is ahead of disk and is what counts;
 *   rebuilding from disk would show older text the next keystroke builds on;
 * - disk differs from the screen → `rebuild` from disk: left alone, the screen would show text disk
 *   no longer holds while saving compares against disk, and the next keystroke would write the old
 *   text back (over a version just restored, say);
 * - otherwise → `reuse`: nothing to change, and the cursor stays where it is.
 */
export function decideReopen(ownEditsPending: boolean, disk: string, live: string): ReopenAction {
  if (ownEditsPending) return "keep";
  return disk === live ? "reuse" : "rebuild";
}
