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
 * Decide what a per-path change to the open note means. A clean note reloads (FS is the truth).
 * A dirty note only has a conflict when disk no longer holds what was last synced — a change event
 * whose content is still that (a late echo, a touch without a content change) is not an external
 * edit, and raising a banner for it would ask the user about their own typing.
 */
export function decideChangeAction(disk: string, dirty: boolean, synced: string): ChangeAction {
  if (!dirty) return "reload";
  return disk === synced ? "none" : "conflict";
}
