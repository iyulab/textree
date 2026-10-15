import { describe, expect, it } from "vitest";
import { readVaultList, vaultListKey, writeVaultList } from "./vaultLocal.helpers";

function memory(): Storage {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => void m.delete(k),
    setItem: (k, v) => void m.set(k, v),
  };
}

describe("lists kept per vault on this device", () => {
  it("each vault keeps its own list, even of notes with the same names", () => {
    const s = memory();
    writeVaultList(s, "recent", "C:\\one", ["a.md"]);
    writeVaultList(s, "recent", "C:\\two", ["b.md"]);
    expect(readVaultList(s, "recent", "c:/one/")).toEqual(["a.md"]);
    expect(readVaultList(s, "recent", "C:\\two")).toEqual(["b.md"]);
  });

  it("the first time, takes this vault's entries from the list kept for all of them", () => {
    const s = memory();
    s.setItem("recent", JSON.stringify(["C:\\one\\a.md", "C:\\two\\b.md", "C:\\one\\sub\\c.md", "C:\\one"]));
    expect(readVaultList(s, "recent", "C:\\one")).toEqual(["a.md", "sub/c.md"]);
    expect(s.getItem("recent")).not.toBeNull(); // the other vault still finds its entries
    expect(s.getItem(vaultListKey("recent", "C:\\one"))).toBeNull(); // nothing written by reading
  });

  it("a damaged list or no storage reads as empty", () => {
    const s = memory();
    s.setItem(vaultListKey("recent", "C:\\one"), "not json");
    s.setItem("recent", "also not");
    expect(readVaultList(s, "recent", "C:\\one")).toEqual([]);
    expect(readVaultList(undefined, "recent", "C:\\one")).toEqual([]);
  });
});
