/*
 * Layout state — sidebar width + collapse.
 *
 * Since it is a presentation-layer preference, it persists in localStorage (app setting —
 * separate from the vault's library schema). Same pattern as theme.svelte.ts.
 */

const WIDTH_KEY = "textree-sidebar-width";
const COLLAPSED_KEY = "textree-sidebar-collapsed";

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX = 480;
const SIDEBAR_DEFAULT = 280;

function clampWidth(w: number): number {
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w));
}

function readWidth(): number {
  if (typeof localStorage === "undefined") return SIDEBAR_DEFAULT;
  const v = Number(localStorage.getItem(WIDTH_KEY));
  return Number.isFinite(v) && v > 0 ? clampWidth(v) : SIDEBAR_DEFAULT;
}

function readCollapsed(): boolean {
  if (typeof localStorage === "undefined") return false;
  return localStorage.getItem(COLLAPSED_KEY) === "true";
}

class LayoutStore {
  width = $state<number>(SIDEBAR_DEFAULT);
  collapsed = $state<boolean>(false);

  // Main-pane mode. Ephemeral (NOT persisted): a restart should reopen in Note
  // mode, never Chat with a vanished session.
  mode = $state<'note' | 'chat'>('note');

  setMode(m: 'note' | 'chat'): void {
    this.mode = m;
  }

  /** Once at app startup: load stored values. */
  init(): void {
    this.width = readWidth();
    this.collapsed = readCollapsed();
  }

  /** Set width during drag (clamped). Persistence happens once at drag end (persistWidth). */
  setWidth(w: number): void {
    this.width = clampWidth(w);
  }

  persistWidth(): void {
    if (typeof localStorage !== "undefined")
      localStorage.setItem(WIDTH_KEY, String(this.width));
  }

  toggleCollapsed(): void {
    this.collapsed = !this.collapsed;
    if (typeof localStorage !== "undefined")
      localStorage.setItem(COLLAPSED_KEY, String(this.collapsed));
  }
}

export const layout = new LayoutStore();
