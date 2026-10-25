/*
 * Tree UI state — collapse set + keyboard focus path.
 *
 * Collapse is a presentation-layer preference, so it persists in localStorage, one list per vault
 * (paths are relative to the vault).
 * Focus (roving tabindex) is session state and does not persist.
 */

import { readVaultList, writeVaultList } from "./vaultLocal.helpers";

const COLLAPSE_KEY = "textree-tree-collapsed";

const storage = () => (typeof localStorage === "undefined" ? undefined : localStorage);

class TreeStore {
  collapsed = $state<Set<string>>(new Set());
  /** The vault whose folders `collapsed` names. */
  private root: string | null = null;
  /** Path of the node under keyboard focus (roving tabindex basis). null = unset (falls back to the first item). */
  focused = $state<string | null>(null);
  /** Path of the node "cut" for keyboard move (Ctrl+X). Moved/adopted onto the paste (Ctrl+V) target. */
  cut = $state<string | null>(null);

  isCollapsed(path: string): boolean {
    return this.collapsed.has(path);
  }

  /** A vault opened: its folders collapse as they were left on this device. */
  load(root: string): void {
    this.root = root;
    this.collapsed = new Set(readVaultList(storage(), COLLAPSE_KEY, root));
    this.focused = null;
    this.cut = null;
  }

  private persist(): void {
    if (this.root !== null) writeVaultList(storage(), COLLAPSE_KEY, this.root, [...this.collapsed]);
  }

  /** A folder moved or was renamed: it stays collapsed, and so do the folders under it. */
  follow(remap: (p: string) => string): void {
    const next = new Set([...this.collapsed].map(remap));
    if (next.size === this.collapsed.size && [...next].every((p) => this.collapsed.has(p))) return;
    this.collapsed = next;
    this.persist();
  }

  collapse(path: string): void {
    if (this.collapsed.has(path)) return;
    const next = new Set(this.collapsed);
    next.add(path);
    this.collapsed = next;
    this.persist();
  }

  expand(path: string): void {
    if (!this.collapsed.has(path)) return;
    const next = new Set(this.collapsed);
    next.delete(path);
    this.collapsed = next;
    this.persist();
  }

  toggle(path: string): void {
    if (this.collapsed.has(path)) this.expand(path);
    else this.collapse(path);
  }

  setFocused(path: string | null): void {
    this.focused = path;
  }

  setCut(path: string | null): void {
    this.cut = path;
  }
}

export const tree = new TreeStore();
