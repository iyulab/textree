import { describe, it, expect } from "vitest";
import type { MoveOut } from "./ipc";
import { lines, reassurance, title, worthShowing } from "./migrationNotice.helpers";

const moved = (over: Partial<MoveOut>): MoveOut => ({
  settings: false,
  notes: 0,
  files: 0,
  leftBehind: [],
  ...over,
});

describe("worthShowing", () => {
  it("says nothing when nothing moved", () => {
    // A notice for a move that did not happen trains people to dismiss notices.
    expect(worthShowing(moved({}))).toBe(false);
  });

  it("speaks up for either kind of move", () => {
    expect(worthShowing(moved({ settings: true }))).toBe(true);
    expect(worthShowing(moved({ notes: 3 }))).toBe(true);
    expect(worthShowing(moved({ files: 1 }))).toBe(true);
  });

  it("speaks up while something is still left behind, even when nothing moved this time", () => {
    expect(worthShowing(moved({ leftBehind: [".textree/trash/.odd"] }))).toBe(true);
  });
});

describe("title", () => {
  it("counts only what moved", () => {
    expect(title(moved({ settings: true, notes: 2 }))).toBe("Two things moved out of your notes folder");
    expect(title(moved({ notes: 2 }))).toBe("Moved out of your notes folder");
    expect(title(moved({ settings: true }))).toBe("Moved out of your notes folder");
    expect(title(moved({ leftBehind: ["x"] }))).toBe("Some files stayed in your notes folder");
  });
});

describe("lines", () => {
  it("mentions only what actually moved", () => {
    expect(lines(moved({ settings: true }))).toHaveLength(1);
    expect(lines(moved({ notes: 2 }))).toEqual(["2 notes from the trash are now under Deleted notes."]);
    expect(lines(moved({ settings: true, notes: 2 }))).toHaveLength(2);
  });

  it("counts one note as one", () => {
    expect(lines(moved({ notes: 1 }))[0]).toBe("1 note from the trash is now under Deleted notes.");
  });

  it("does not call an attachment a note", () => {
    expect(lines(moved({ notes: 3, files: 1 }))[0]).toBe(
      "3 notes and 1 other file from the trash are now under Deleted notes.",
    );
    expect(lines(moved({ files: 2 }))[0]).toBe("2 files from the trash are now under Deleted notes.");
  });

  it("names what stayed behind, and counts the rest", () => {
    expect(lines(moved({ leftBehind: ["a", "b"] }))).toEqual([
      "These could not be moved and are still in the folder, untouched: a, b.",
    ]);
    expect(lines(moved({ leftBehind: ["a", "b", "c", "d", "e"] }))[0]).toContain("a, b, c, and 2 more.");
  });
});

describe("reassurance", () => {
  it("says nothing was removed, and says the part that only shows up later", () => {
    // Going back to an older version leaves the favourites behind. Finding that out by
    // downgrading is worse than being told now.
    const said = reassurance(moved({ settings: true, notes: 1 }));
    expect(said).toContain("Nothing was removed");
    expect(said).toContain("only your notes");
    expect(said).toContain("older version");
  });

  it("never calls the folder clean while something stayed in it", () => {
    const said = reassurance(moved({ notes: 1, leftBehind: [".textree/trash/stray.md"] }));
    expect(said).toContain("Nothing was removed");
    expect(said).not.toContain("only your notes");
  });

  it("does not mention favourites when none moved", () => {
    expect(reassurance(moved({ notes: 2 }))).not.toContain("favourites");
  });
});
