/*
 * Command registry — targets the unified palette's '>' command mode.
 * Builds the command list from injected component-owned actions (minimizing coupling).
 */

export interface Command {
  id: string;
  title: string;
  /** Menu category this command belongs to; must be a value in MENU_CATEGORIES. */
  category: string;
  run: () => void | Promise<void>;
  /** If false, inactive in the current context (excluded from the list). Always active if omitted. */
  when?: () => boolean;
  /** Global accelerator (e.g. "mod+n"); wired by the global key handler and shown in the palette. */
  keybinding?: string;
}

/** Bundle of app actions the palette invokes. +page.svelte implements and injects them. */
export interface PaletteActions {
  openVault: () => void;
  toggleTheme: () => void;
  toggleSidebar: () => void;
  toggleReading: () => void;
  /** Switch between Note and Chat main-pane modes (entering Chat ensures a session). */
  toggleMode: () => void;
  newNoteAtRoot: () => void;
  newFolderAtRoot: () => void;
  hasSelection: () => boolean;
  renameSelected: () => void;
  deleteSelected: () => void;
  promoteSelected: () => void;
  toggleFavoriteSelected: () => void;
  moveSelectedUp: () => void;
  moveSelectedDown: () => void;
  rebuildIndex: () => void;
  hasVault: () => boolean;
  /** Whether a note is open, so a version of it can be added. */
  hasOpenNote: () => boolean;
  addVersion: () => void;
  publishSite: () => void;
  publishToWeb: () => void;
  openTrash: () => void;
  openLogDir: () => void;
  openSettings: () => void;
}

/** Ordered categories for the ⋮ app menu (also the group render order). */
export const MENU_CATEGORIES = [
  "Vault",
  "Create",
  "Selected node",
  "View",
  "Search",
  "Help",
] as const;

export function buildCommands(a: PaletteActions): Command[] {
  const sel = a.hasSelection;
  return [
    { id: "vault.open", title: "Open / switch vault", category: "Vault", run: a.openVault },
    { id: "view.theme", title: "Toggle theme (light/dark)", category: "View", run: a.toggleTheme },
    { id: "view.settings", title: "Settings", category: "View", run: a.openSettings, keybinding: "mod+," },
    { id: "view.sidebar", title: "Toggle sidebar", category: "View", run: a.toggleSidebar },
    { id: "view.reading", title: "Toggle reading view", category: "View", run: a.toggleReading },
    { id: "view.modeToggle", title: "Toggle Note / Chat", category: "View", run: a.toggleMode, keybinding: "mod+shift+m", when: a.hasVault },
    { id: "note.new", title: "New note (root)", category: "Create", run: a.newNoteAtRoot, keybinding: "mod+n" },
    { id: "folder.new", title: "New folder (root)", category: "Create", run: a.newFolderAtRoot, keybinding: "mod+shift+n" },
    { id: "node.rename", title: "Rename selected node", category: "Selected node", run: a.renameSelected, when: sel },
    { id: "node.delete", title: "Delete selected node", category: "Selected node", run: a.deleteSelected, when: sel },
    { id: "node.promote", title: "Promote selected node", category: "Selected node", run: a.promoteSelected, when: sel },
    { id: "node.favorite", title: "Toggle favorite on selected node", category: "Selected node", run: a.toggleFavoriteSelected, when: sel },
    { id: "node.moveUp", title: "Move selected node up", category: "Selected node", run: a.moveSelectedUp, when: sel },
    { id: "node.moveDown", title: "Move selected node down", category: "Selected node", run: a.moveSelectedDown, when: sel },
    { id: "note.addVersion", title: "Add version…", category: "Selected node", run: a.addVersion, keybinding: "mod+shift+s", when: a.hasOpenNote },
    { id: "search.rebuild", title: "Rebuild content index", category: "Search", run: a.rebuildIndex },
    { id: "vault.publish", title: "Publish site…", category: "Vault", run: a.publishSite, when: a.hasVault },
    { id: "vault.publishWeb", title: "Publish to web", category: "Vault", run: a.publishToWeb, when: a.hasVault },
    { id: "vault.trash", title: "Trash…", category: "Vault", run: a.openTrash, when: a.hasVault },
    { id: "log.openDir", title: "Open log folder", category: "Help", run: a.openLogDir },
  ];
}

/** Evaluates when() and returns only currently active commands. */
export function activeCommands(cmds: Command[]): Command[] {
  return cmds.filter((c) => c.when === undefined || c.when());
}
