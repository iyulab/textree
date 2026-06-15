// Sync Coordinator — coordinates the backend `fs_changed` event with UI state (design §4).
//
// Responsibility: receive external file changes to (1) refresh the tree, and (2) when the open note
// is affected, branch into reload/removal-mark/conflict-banner per the conflict policy (design §4.2).
// The state itself is held by the page; here we only coordinate via injected handlers.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { listTree, readNote, type TreeNode } from "./ipc";

export type FsChangeKind = "created" | "modified" | "removed";

export interface FsChange {
  kind: FsChangeKind;
  path: string;
}

export interface SyncHandlers {
  /** Current vault root (null if none). */
  root: () => string | null;
  /** Body path of the currently open note (null if none). */
  activePath: () => string | null;
  /** Whether the editor has unsaved edits. */
  isDirty: () => boolean;
  /** Replace with the refreshed tree. */
  setTree: (tree: TreeNode[]) => void;
  /** Reload a clean note with the disk content (FS is the truth). */
  reloadActive: (diskContent: string) => void;
  /** The open note was removed/moved externally. */
  activeRemoved: () => void;
  /** External change while dirty — passes the disk content for non-destructive conflict resolution. */
  conflict: (diskContent: string) => void;
}

/** Normalization for path comparison. Absorbs separator differences and compares in lowercase
 *  to match Windows case-insensitivity (Windows-first; same policy as pathInside in +page). */
function samePath(a: string, b: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, "/").toLowerCase();
  return norm(a) === norm(b);
}

/** Handle a single fs change (refresh tree + apply to the open note). */
async function handleChange(handlers: SyncHandlers, payload: FsChange): Promise<void> {
  const root = handlers.root();
  if (!root) return;

  // 1) Refresh the tree on any change (reflect create/remove/rename).
  try {
    handlers.setTree(await listTree(root));
  } catch {
    // A tree refresh failure is non-fatal — recovered on the next event.
  }

  // 2) Process the body only if the open note is affected.
  const active = handlers.activePath();
  if (!active || !samePath(payload.path, active)) return;

  if (payload.kind === "removed") {
    handlers.activeRemoved();
    return;
  }

  // created/modified: read the disk content and apply the conflict policy.
  let disk: string;
  try {
    disk = await readNote(root, active);
  } catch {
    handlers.activeRemoved(); // read failure = effectively gone
    return;
  }

  if (handlers.isDirty()) {
    handlers.conflict(disk); // protect unsaved edits — user chooses
  } else {
    handlers.reloadActive(disk); // reload silently
  }
}

/**
 * Start the `fs_changed` subscription. Detach via the returned unlisten.
 * Serializes handlers into a promise chain to prevent ordering inversions where, under concurrent
 * execution, stale `listTree`/`readNote` results overwrite the latest results.
 */
export async function startSync(handlers: SyncHandlers): Promise<UnlistenFn> {
  let chain: Promise<void> = Promise.resolve();
  return listen<FsChange>("fs_changed", ({ payload }) => {
    chain = chain.then(() => handleChange(handlers, payload)).catch(() => {});
  });
}
