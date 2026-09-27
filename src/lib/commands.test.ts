import { describe, it, expect, vi } from "vitest";
import { buildCommands, MENU_CATEGORIES, type PaletteActions } from "./commands";

/** Every action stubbed to a no-op; override specific ones per test. */
function stubActions(over: Partial<PaletteActions> = {}): PaletteActions {
  const keys: (keyof PaletteActions)[] = [
    "openVault", "toggleTheme", "toggleSidebar", "toggleReading", "toggleMode",
    "newNoteAtRoot", "newFolderAtRoot", "hasSelection", "renameSelected",
    "deleteSelected", "promoteSelected", "toggleFavoriteSelected", "moveSelectedUp",
    "moveSelectedDown", "rebuildIndex", "hasVault", "hasOpenNote", "addVersion", "openVersionHistory", "startAlternative",
    "publishSite", "publishToWeb", "openDeletedNotes",
    "openLogDir", "openSettings",
  ];
  const base = Object.fromEntries(keys.map((k) => [k, () => {}]));
  return { ...base, ...over } as PaletteActions;
}

describe("buildCommands — Note/Chat toggle", () => {
  it("registers view.modeToggle with the mod+shift+m accelerator", () => {
    const cmd = buildCommands(stubActions()).find((c) => c.id === "view.modeToggle");
    expect(cmd).toBeDefined();
    expect(cmd!.keybinding).toBe("mod+shift+m");
  });

  it("gates the toggle behind an open vault (when=hasVault)", () => {
    const hasVault = vi.fn(() => false);
    const cmd = buildCommands(stubActions({ hasVault })).find((c) => c.id === "view.modeToggle")!;
    expect(cmd.when).toBeDefined();
    expect(cmd.when!()).toBe(false);
  });

  it("runs the toggleMode action", () => {
    const toggleMode = vi.fn();
    const cmd = buildCommands(stubActions({ toggleMode })).find((c) => c.id === "view.modeToggle")!;
    cmd.run();
    expect(toggleMode).toHaveBeenCalledOnce();
  });
});

describe("buildCommands — menu categories", () => {
  it("every command declares a category within MENU_CATEGORIES", () => {
    const cmds = buildCommands(stubActions());
    const allowed = new Set<string>(MENU_CATEGORIES);
    expect(cmds.length).toBeGreaterThan(0);
    for (const c of cmds) {
      expect(c.category, `command ${c.id} must have a category`).toBeDefined();
      expect(allowed.has(c.category), `command ${c.id} category "${c.category}"`).toBe(true);
    }
  });
});

describe("buildCommands — Add version", () => {
  it("is offered only while a note is open", () => {
    const withNote = buildCommands(stubActions({ hasOpenNote: () => true }));
    const without = buildCommands(stubActions({ hasOpenNote: () => false }));
    const find = (cmds: ReturnType<typeof buildCommands>) =>
      cmds.find((c) => c.id === "note.addVersion");

    expect(find(withNote)?.when?.()).toBe(true);
    expect(find(without)?.when?.()).toBe(false);
  });

  it("carries an accelerator and the agreed wording", () => {
    const cmd = buildCommands(stubActions()).find((c) => c.id === "note.addVersion");
    expect(cmd?.title).toBe("Add version…");
    expect(cmd?.keybinding).toBe("mod+shift+s");
  });

  it("runs the action it was given", () => {
    const addVersion = vi.fn();
    buildCommands(stubActions({ addVersion }))
      .find((c) => c.id === "note.addVersion")!
      .run();
    expect(addVersion).toHaveBeenCalledOnce();
  });
});

describe("buildCommands — Version history", () => {
  it("is offered only while a note is open, and carries the agreed wording", () => {
    const cmd = buildCommands(stubActions({ hasOpenNote: () => true })).find(
      (c) => c.id === "note.versionHistory",
    );
    expect(cmd?.title).toBe("Version history…");
    expect(cmd?.when?.()).toBe(true);
  });
});

describe("buildCommands — Deleted notes", () => {
  it("is offered whenever a vault is open, with the agreed wording", () => {
    const cmd = buildCommands(stubActions({ hasVault: () => true })).find(
      (c) => c.id === "vault.deleted",
    );
    expect(cmd?.title).toBe("Deleted notes…");
    expect(cmd?.when?.()).toBe(true);
  });
});
