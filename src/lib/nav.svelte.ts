/*
 * Navigation store — favorites, recent, manual order.
 * Favorites/order live in .textree/ (vault-bound); recent lives in localStorage (device-bound).
 *
 * Pure logic (mergeOrder/dedupePushFront/RECENT_MAX) lives in nav.helpers.ts —
 * split into a runes-free module so it can be tested directly under vitest (node environment).
 */

import { readSidecar, writeSidecar } from "./ipc";
import { dedupePushFront, RECENT_MAX } from "./nav.helpers";

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

  /** Called on vault open/switch — reloads favorites/order from the sidecar. */
  async load(root: string): Promise<void> {
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
    await this.persist("favorites.json", this.favorites);
  }

  pushRecent(path: string): void {
    this.recent = dedupePushFront(this.recent, path, RECENT_MAX);
    if (typeof localStorage !== "undefined")
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
  }

  async setOrder(parent: string, paths: string[]): Promise<void> {
    this.order = { ...this.order, [parent]: paths };
    await this.persist("order.json", this.order);
  }

  private async persist(rel: string, value: unknown): Promise<void> {
    if (this.root === null) return;
    try {
      await writeSidecar(this.root, rel, JSON.stringify(value));
    } catch (e) {
      // Non-blocking: keep the in-memory state. Retry recovery on the next write.
      console.warn(`사이드카 쓰기 실패(${rel}):`, e);
    }
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
    console.warn(`사이드카 읽기/파싱 실패(${rel}):`, e);
    return null;
  }
}

export const nav = new NavStore();
