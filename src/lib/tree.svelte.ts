/*
 * Tree UI state — collapse set + keyboard focus path.
 *
 * Collapse is a presentation-layer preference, so it persists in localStorage
 * (based on absolute paths → no vault conflict).
 * Focus (roving tabindex) is session state and does not persist.
 */

const COLLAPSE_KEY = "textree-tree-collapsed";

function loadCollapsed(): Set<string> {
  if (typeof localStorage === "undefined") return new Set();
  try {
    const arr = JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? "[]");
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

class TreeStore {
  collapsed = $state<Set<string>>(loadCollapsed());
  /** Path of the node under keyboard focus (roving tabindex basis). null = unset (falls back to the first item). */
  focused = $state<string | null>(null);
  /** Path of the node "cut" for keyboard move (Ctrl+X). Moved/adopted onto the paste (Ctrl+V) target. */
  cut = $state<string | null>(null);

  isCollapsed(path: string): boolean {
    return this.collapsed.has(path);
  }

  private persist(): void {
    if (typeof localStorage !== "undefined")
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...this.collapsed]));
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
