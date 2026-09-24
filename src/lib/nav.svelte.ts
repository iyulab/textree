/*
 * Navigation store — favorites, recent, manual order.
 * Favorites/order are kept with the app per vault; recent lives in localStorage (device-bound).
 *
 * Pure logic (mergeOrder/dedupePushFront/RECENT_MAX) lives in nav.helpers.ts —
 * split into a runes-free module so it can be tested directly under vitest (node environment).
 */

import { readSidecar, writeSidecar } from "./ipc";
import { dedupePushFront, RECENT_MAX } from "./nav.helpers";
import { RetryingWriter } from "./sidecarWriter.helpers";

// Re-export so components can import the order helper alongside from `$lib/nav.svelte`.
export { mergeOrder } from "./nav.helpers";

const RECENT_KEY = "textree-recent";

class NavStore {
  favorites = $state<string[]>([]);
  recent = $state<string[]>(loadRecent());
  /** parentPath → child path order. */
  order = $state<Record<string, string[]>>({});
  /** Current vault root (target of persist IPC). null means not loaded. */
  private root: string | null = null;
  // Favorites and order are separate files: a failed write of one is retried when the other is
  // written, not only when the same one is.
  private writer = new RetryingWriter(
    (rel, body) => (this.root === null ? Promise.resolve() : writeSidecar(this.root, rel, body)),
    // Non-blocking: the in-memory state stays as the person left it.
    (rel, e) => console.warn(`Sidecar write failed (${rel}):`, e),
  );

  /** Called on vault open/switch — reloads favorites/order from the sidecar. */
  async load(root: string): Promise<void> {
    this.writer.reset(); // owed writes belong to the folder being left
    this.root = root;
    this.favorites = (await readJson<string[]>(root, "favorites.json")) ?? [];
    this.order = (await readJson<Record<string, string[]>>(root, "order.json")) ?? {};
  }

  isFavorite(path: string): boolean {
    return this.favorites.includes(path);
  }

  async toggleFavorite(path: string): Promise<void> {
    this.favorites = this.isFavorite(path)
      ? this.favorites.filter((p) => p !== path)
      : [...this.favorites, path];
    await this.writer.save("favorites.json", () => JSON.stringify(this.favorites));
  }

  pushRecent(path: string): void {
    this.recent = dedupePushFront(this.recent, path, RECENT_MAX);
    if (typeof localStorage !== "undefined")
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
  }

  async setOrder(parent: string, paths: string[]): Promise<void> {
    this.order = { ...this.order, [parent]: paths };
    await this.writer.save("order.json", () => JSON.stringify(this.order));
  }

}

function loadRecent(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const arr = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
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

export const nav = new NavStore();
