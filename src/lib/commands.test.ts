import { describe, it, expect, vi } from "vitest";
import { buildCommands, type PaletteActions } from "./commands";

/** Every action stubbed to a no-op; override specific ones per test. */
function stubActions(over: Partial<PaletteActions> = {}): PaletteActions {
  const keys: (keyof PaletteActions)[] = [
    "openVault", "toggleTheme", "toggleSidebar", "toggleReading", "toggleMode",
    "newNoteAtRoot", "newFolderAtRoot", "hasSelection", "renameSelected",
    "deleteSelected", "promoteSelected", "toggleFavoriteSelected", "moveSelectedUp",
    "moveSelectedDown", "rebuildIndex", "hasVault", "publishSite", "openTrash",
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
