/*
 * Theme state — light/dark/system (auto).
 *
 * The selection persists in localStorage("textree-theme"). This is an **app setting**,
 * separate from the vault's library schema (.textree/, frontmatter) — a presentation-layer
 * preference, not note data.
 *
 * "auto" tracks the OS preference (prefers-color-scheme) in real time. Explicit light/dark
 * ignores OS changes. The actual application happens via the <html data-theme> attribute, and
 * the [data-theme="dark"] selector in tokens.css switches the colors.
 */

export type ThemeMode = "auto" | "light" | "dark";

const STORAGE_KEY = "textree-theme";

function readStored(): ThemeMode {
  if (typeof localStorage === "undefined") return "auto";
  const v = localStorage.getItem(STORAGE_KEY);
  return v === "light" || v === "dark" || v === "auto" ? v : "auto";
}

function systemPrefersDark(): boolean {
  return (
    typeof matchMedia !== "undefined" &&
    matchMedia("(prefers-color-scheme: dark)").matches
  );
}

/** Resolves mode to the actual dark/light value. */
function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "auto") return systemPrefersDark() ? "dark" : "light";
  return mode;
}

/** Reflects the resolved theme onto <html data-theme>. */
function apply(mode: ThemeMode): void {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", resolve(mode));
}

class ThemeStore {
  /** User-selected mode (auto/light/dark). */
  mode = $state<ThemeMode>("auto");
  /** Currently applied theme (derived). */
  resolved = $derived<"light" | "dark">(resolve(this.mode));

  private mql: MediaQueryList | null = null;
  private onSystemChange = () => {
    // Reflect OS changes only when auto (explicit selection stays fixed).
    if (this.mode === "auto") apply(this.mode);
  };

  /** Once at app startup: load the stored selection, reflect it to the DOM, and subscribe to OS changes.
   *  Returns a cleanup function (unsubscribe). */
  init(): () => void {
    this.mode = readStored();
    apply(this.mode);
    if (typeof matchMedia !== "undefined") {
      this.mql = matchMedia("(prefers-color-scheme: dark)");
      this.mql.addEventListener("change", this.onSystemChange);
    }
    return () => this.mql?.removeEventListener("change", this.onSystemChange);
  }

  /** Set mode + persist + apply. */
  set(mode: ThemeMode): void {
    this.mode = mode;
    if (typeof localStorage !== "undefined") localStorage.setItem(STORAGE_KEY, mode);
    apply(mode);
  }

  /** Toggle light ↔ dark. If it was auto, switch to a fixed value opposite the current resolved value. */
  toggle(): void {
    this.set(this.resolved === "dark" ? "light" : "dark");
  }
}

export const theme = new ThemeStore();
