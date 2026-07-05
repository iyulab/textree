// Pure decision logic for the Sync Coordinator (sync.ts) — vitest imports this directly.

export type RescanAction = "none" | "reload" | "conflict";

/**
 * Reconcile the open note after a watcher rescan (the OS dropped events, so the note MAY have
 * changed externally with no per-path event). Unlike a per-path `fs_changed` — which is evidence
 * of a real external change — a rescan carries no evidence about this note, so the disk content
 * is compared against what the user currently sees (`live`) first:
 *
 * - identical → nothing to do (no editor reload, no cursor/focus disturbance, no false banner);
 * - differs while clean → silent reload (FS is the truth);
 * - differs while dirty → conflict banner. This can be a benign mid-edit state (disk still holds
 *   the pre-autosave text), but a dropped external change is indistinguishable from it here, and
 *   a non-destructive banner beats silently overwriting that change on the next autosave.
 */
export function decideRescanAction(disk: string, live: string, dirty: boolean): RescanAction {
  if (disk === live) return "none";
  return dirty ? "conflict" : "reload";
}
