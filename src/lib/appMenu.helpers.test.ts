import { describe, it, expect } from "vitest";
import { groupCommands } from "./appMenu.helpers";
import type { Command } from "./commands";

function cmd(id: string, category: string, over: Partial<Command> = {}): Command {
  return { id, title: id, category, run: () => {}, ...over };
}

describe("groupCommands", () => {
  it("orders groups by MENU_CATEGORIES, not input order", () => {
    const groups = groupCommands([
      cmd("search.x", "Search"),
      cmd("vault.x", "Vault"),
      cmd("view.x", "View"),
    ]);
    expect(groups.map((g) => g.category)).toEqual(["Vault", "View", "Search"]);
  });

  it("buckets multiple commands under the same category in input order", () => {
    const groups = groupCommands([cmd("a", "Vault"), cmd("b", "Vault")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].items.map((c) => c.id)).toEqual(["a", "b"]);
  });

  it("drops empty categories", () => {
    const groups = groupCommands([cmd("a", "Vault")]);
    expect(groups.map((g) => g.category)).toEqual(["Vault"]);
  });

  it("excludes commands inactive by when()", () => {
    const groups = groupCommands([
      cmd("a", "Selected node", { when: () => false }),
      cmd("b", "Vault"),
    ]);
    expect(groups.map((g) => g.category)).toEqual(["Vault"]);
  });
});
