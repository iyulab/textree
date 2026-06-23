/*
 * Command registry — targets the unified palette's '>' command mode.
 * Builds the command list from injected component-owned actions (minimizing coupling).
 */

export interface Command {
  id: string;
  title: string;
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
  publishSite: () => void;
  openTrash: () => void;
  openLogDir: () => void;
}

export function buildCommands(a: PaletteActions): Command[] {
  const sel = a.hasSelection;
  return [
    { id: "vault.open", title: "Open / switch vault", run: a.openVault },
    { id: "view.theme", title: "Toggle theme (light/dark)", run: a.toggleTheme },
    { id: "view.sidebar", title: "Toggle sidebar", run: a.toggleSidebar },
    { id: "view.reading", title: "Toggle reading view", run: a.toggleReading },
    { id: "note.new", title: "New note (root)", run: a.newNoteAtRoot, keybinding: "mod+n" },
    { id: "folder.new", title: "New folder (root)", run: a.newFolderAtRoot, keybinding: "mod+shift+n" },
    { id: "node.rename", title: "Rename selected node", run: a.renameSelected, when: sel },
    { id: "node.delete", title: "Delete selected node", run: a.deleteSelected, when: sel },
    { id: "node.promote", title: "Promote selected node", run: a.promoteSelected, when: sel },
    { id: "node.favorite", title: "Toggle favorite on selected node", run: a.toggleFavoriteSelected, when: sel },
    { id: "node.moveUp", title: "Move selected node up", run: a.moveSelectedUp, when: sel },
    { id: "node.moveDown", title: "Move selected node down", run: a.moveSelectedDown, when: sel },
    { id: "search.rebuild", title: "Rebuild content index", run: a.rebuildIndex },
    { id: "vault.publish", title: "Publish site…", run: a.publishSite, when: a.hasVault },
    { id: "vault.trash", title: "Trash…", run: a.openTrash, when: a.hasVault },
    { id: "log.openDir", title: "Open log folder", run: a.openLogDir },
  ];
}

/** Evaluates when() and returns only currently active commands. */
export function activeCommands(cmds: Command[]): Command[] {
  return cmds.filter((c) => c.when === undefined || c.when());
}
