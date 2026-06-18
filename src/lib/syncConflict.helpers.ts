/*
 * Sync-conflict detection — pure scan of the loaded tree (no new IPC, no backend).
 *
 * When a vault lives in a sync folder (OneDrive/Dropbox/Syncthing) and the same note is
 * edited on two devices, the sync client keeps both versions and renames one with a
 * conflict marker. Those renamed files are valid `.md` files, so they surface in the tree
 * as ordinary notes the user may never notice — a silent path to losing edits.
 *
 * This helper flags them so the app can surface them non-destructively (no auto-merge,
 * no auto-delete — constitution D18 data-safety guard). The user decides.
 *
 * Precision over recall: we only match markers that are distinctive enough to be safe to
 * flag. Ambiguous patterns (OneDrive's `-MACHINENAME`, Google's ` (1)`, iCloud's ` 2`) are
 * deliberately NOT flagged — a false positive on a legitimately named note erodes trust in
 * the warning. Missing a rare conflict is better than crying wolf on a real note.
 */

import type { TreeNode } from "./ipc";

export type SyncConflictSource = "dropbox" | "syncthing";

export interface SyncConflict {
  /** Filesystem path of the conflict-copy node. */
  path: string;
  /** Display name (file stem) of the node. */
  name: string;
  /** Which sync client's marker was matched. */
  source: SyncConflictSource;
}

// Dropbox: `name (Someone's conflicted copy 2026-06-18)`. The stable, locale-independent-enough
// marker across versions is the phrase "conflicted copy". (OneDrive does NOT use this wording — it
// appends the machine name, `name-COMPUTERNAME.ext`, which is too ambiguous to flag safely; see the
// precision-over-recall note above. So OneDrive's conflict copies are intentionally not surfaced.)
const DROPBOX_MARKER = /conflicted copy/i;
// Syncthing: `name.sync-conflict-YYYYMMDD-HHMMSS-DEVICEID.ext`.
const SYNCTHING_MARKER = /\.sync-conflict-\d{8}-\d{6}/i;

function classify(name: string): SyncConflictSource | null {
  if (SYNCTHING_MARKER.test(name)) return "syncthing";
  if (DROPBOX_MARKER.test(name)) return "dropbox";
  return null;
}

/**
 * Walks the tree (leaves and containers alike) and returns every node whose name carries a
 * recognised sync-conflict marker, flattened in pre-order.
 */
export function detectSyncConflicts(nodes: TreeNode[]): SyncConflict[] {
  const out: SyncConflict[] = [];
  const walk = (list: TreeNode[]): void => {
    for (const node of list) {
      const source = classify(node.name);
      if (source) out.push({ path: node.path, name: node.name, source });
      if (node.children.length) walk(node.children);
    }
  };
  walk(nodes);
  return out;
}
