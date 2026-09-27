/**
 * Pure text for the always-visible backup indicator next to a note's title.
 *
 * Versions live only in this repository's own refs, so until notes reach another machine this
 * computer holds the only copy. The indicator says so for as long as it stays true — it is a
 * standing fact, not a one-time notice.
 *
 * An existing `origin` does not count: a default push sends branches, not the refs versions are
 * kept in. "Backed up" means the folder's remote held those refs at the last exchange.
 */

export interface BackupStatus {
  label: string;
  tooltip: string;
}

export const NOT_BACKED_UP: BackupStatus = {
  label: "Not backed up",
  tooltip:
    "Your notes and their versions are only on this computer. If this disk fails, they can't be recovered.",
};

/** What the indicator shows, or `null` when there is nothing to warn about. */
export function backupStatus(notesBackedUp: boolean): BackupStatus | null {
  return notesBackedUp ? null : NOT_BACKED_UP;
}
