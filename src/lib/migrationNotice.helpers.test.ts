import { describe, it, expect } from "vitest";
import { lines, reassurance, worthShowing } from "./migrationNotice.helpers";

describe("worthShowing", () => {
  it("says nothing when nothing moved", () => {
    // A notice for a move that did not happen trains people to dismiss notices.
    expect(worthShowing({ settings: false, notes: 0 })).toBe(false);
  });

  it("speaks up for either kind of move", () => {
    expect(worthShowing({ settings: true, notes: 0 })).toBe(true);
    expect(worthShowing({ settings: false, notes: 3 })).toBe(true);
  });
});

describe("lines", () => {
  it("mentions only what actually moved", () => {
    expect(lines({ settings: true, notes: 0 })).toHaveLength(1);
    expect(lines({ settings: false, notes: 2 })).toEqual([
      "2 notes from the trash are now under Deleted notes.",
    ]);
    expect(lines({ settings: true, notes: 2 })).toHaveLength(2);
  });

  it("counts one note as one", () => {
    expect(lines({ settings: false, notes: 1 })[0]).toContain("1 note from");
  });
});

describe("reassurance", () => {
  it("says nothing was removed, and says the part that only shows up later", () => {
    // Going back to an older version leaves the favourites behind. Finding that out by
    // downgrading is worse than being told now.
    expect(reassurance()).toContain("Nothing was removed");
    expect(reassurance()).toContain("older version");
  });
});
