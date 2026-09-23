import { describe, expect, it } from "vitest";
import {
  folderBody,
  noteStem,
  pathInside,
  remapAdopted,
  remapMoved,
  remapPromoted,
  remapRenamed,
  samePath,
} from "./vaultPaths.helpers";

const V = "C:\\vault";

describe("vault paths", () => {
  it("compare ignoring separators, a trailing slash and case", () => {
    expect(samePath(`${V}\\A\\n.md`, "c:/vault/a/N.md")).toBe(true);
    expect(pathInside(`${V}\\a\\n.md`, `${V}\\a\\`)).toBe(true);
    expect(pathInside(`${V}\\ab\\n.md`, `${V}\\a`)).toBe(false);
  });

  it("name a note by its file and a folder's body by the folder", () => {
    expect(noteStem(`${V}\\a\\Note.MD`)).toBe("Note");
    expect(folderBody(`${V}\\a`)).toBe(`${V}\\a\\a.md`);
  });
});

describe("where a structure change puts a path", () => {
  it("a move carries everything under the moved node and nothing else", () => {
    const r = remapMoved(`${V}\\a`, `${V}\\dest\\a`);
    expect(r(`${V}\\a\\x.md`)).toBe(`${V}\\dest\\a\\x.md`);
    expect(r(`${V}\\ab.md`)).toBe(`${V}\\ab.md`);
  });

  it("renaming a note moves only that note", () => {
    const r = remapRenamed(`${V}\\old.md`, `${V}\\new.md`, false);
    expect(r(`${V}\\old.md`)).toBe(`${V}\\new.md`);
    expect(r(`${V}\\other.md`)).toBe(`${V}\\other.md`);
  });

  it("renaming a folder renames its body note with it and keeps the other notes' names", () => {
    const r = remapRenamed(`${V}\\old`, `${V}\\new`, true);
    expect(r(`${V}\\old\\old.md`)).toBe(`${V}\\new\\new.md`);
    expect(r(`${V}\\old\\child.md`)).toBe(`${V}\\new\\child.md`);
    expect(r(`${V}\\old\\sub\\old.md`)).toBe(`${V}\\new\\sub\\old.md`);
  });

  it("a promoted note becomes its folder's body", () => {
    const r = remapPromoted(`${V}\\leaf.md`, `${V}\\leaf`);
    expect(r(`${V}\\leaf.md`)).toBe(`${V}\\leaf\\leaf.md`);
    expect(r(`${V}\\other.md`)).toBe(`${V}\\other.md`);
  });

  it("adopting moves the dropped note into the promoted one, which becomes the body", () => {
    const r = remapAdopted(`${V}\\guest.md`, `${V}\\host.md`, `${V}\\host\\guest.md`);
    expect(r(`${V}\\host.md`)).toBe(`${V}\\host\\host.md`);
    expect(r(`${V}\\guest.md`)).toBe(`${V}\\host\\guest.md`);
    expect(r(`${V}\\other.md`)).toBe(`${V}\\other.md`);
  });
});
