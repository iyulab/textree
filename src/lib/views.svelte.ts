/*
 * Saved-views store — folder → named views, persisted to .textree/views.json (vault-bound).
 *
 * Persistence shape mirrors order.json (nav.svelte.ts): a single fixed sidecar file keyed by folder
 * path. This sidesteps enumeration (one read, filter by folder in memory), makes delete clean (drop
 * from the array — no orphan files), and uses folder paths as JSON keys (not filenames), so paths
 * like `a/b` and `a-b` never collide. New IPC 0 (reuses write_sidecar/read_sidecar).
 *
 * NOTE — deviation from the ratified `views/<slug>.json` per-view-file record: the ratified
 * load-bearing decisions (6-field schema, .textree/ sidecar, versioned JSON, file-is-truth, NEW IPC
 * 0, interop trade-off) are all preserved; only the container changes (per-view file → folder-keyed
 * map), because honoring per-view files literally would need a list/delete IPC and break "new IPC 0".
 * The ViewDefinition schema (view.helpers.ts) is untouched and stored as the map's array values.
 *
 * Pure logic (upsertView/removeView) lives in view.helpers.ts (vitest); this runes module is a thin
 * persistence wrapper and is not imported by tests (constitution: pure ↔ runes separation).
 */

import { readSidecar, writeSidecar } from "./ipc";
import { removeView, upsertView, type ViewDefinition } from "./view.helpers";

const VIEWS_FILE = "views.json";

class ViewsStore {
  /** folderPath → that folder's saved views. */
  private all = $state<Record<string, ViewDefinition[]>>({});
  /** Current vault root (target of persist IPC). null means not loaded. */
  private root: string | null = null;

  /** Called on vault open/switch — reloads saved views from the sidecar. */
  async load(root: string): Promise<void> {
    this.root = root;
    this.all = (await readJson<Record<string, ViewDefinition[]>>(root, VIEWS_FILE)) ?? {};
  }

  /** Saved views for a folder, in saved order (empty if none). */
  forFolder(folder: string): ViewDefinition[] {
    return this.all[folder] ?? [];
  }

  /** Save (insert or replace by name) a view in its folder. The view's `folder` field is the key. */
  async save(view: ViewDefinition): Promise<void> {
    const next = upsertView(this.all[view.folder] ?? [], view);
    this.all = { ...this.all, [view.folder]: next };
    await this.persist();
  }

  /** Remove a named view from a folder. Drops the folder key entirely when it empties. */
  async remove(folder: string, name: string): Promise<void> {
    const next = removeView(this.all[folder] ?? [], name);
    const all = { ...this.all };
    if (next.length) all[folder] = next;
    else delete all[folder];
    this.all = all;
    await this.persist();
  }

  private async persist(): Promise<void> {
    if (this.root === null) return;
    try {
      await writeSidecar(this.root, VIEWS_FILE, JSON.stringify(this.all));
    } catch (e) {
      // Non-blocking: keep the in-memory state. Retry recovery on the next write.
      console.warn(`Sidecar write failed (${VIEWS_FILE}):`, e);
    }
  }
}

/** Reads sidecar JSON — null on absence/corruption (the caller falls back to a default). */
async function readJson<T>(root: string, rel: string): Promise<T | null> {
  try {
    const raw = await readSidecar(root, rel);
    return raw === null ? null : (JSON.parse(raw) as T);
  } catch (e) {
    console.warn(`Sidecar read/parse failed (${rel}):`, e);
    return null;
  }
}

export const views = new ViewsStore();
