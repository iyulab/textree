import { describe, it, expect } from "vitest";
import {
  containingFolder,
  displayName,
  emptyMessage,
  formatDeletedAt,
  restoredMessage,
  sortDeleted,
  standingNote,
} from "./deletedNotes.helpers";
import type { DeletedNote } from "./ipc";

function note(rel: string, seconds: number, recorded = true): DeletedNote {
  return { rel, seconds, recorded };
}

describe("displayName / containingFolder", () => {
  it("reads the name the way the screen shows it", () => {
    expect(displayName("sub/deep/Plan draft.md")).toBe("Plan draft");
    expect(containingFolder("sub/deep/Plan draft.md")).toBe("sub/deep");
  });

  it("has no folder to name for a note at the top", () => {
    expect(containingFolder("a.md")).toBeNull();
  });

  it("keeps names that are not written in the Latin alphabet", () => {
    // A name outside the Latin alphabet, so that the slice is by character and not by byte.
    // forbidden-tokens: allow non-latin - the multi-byte name is what this case is about
    expect(displayName("sub/메모.md")).toBe("메모");
  });
});

describe("sortDeleted", () => {
  it("puts the most recently deleted first", () => {
    const sorted = sortDeleted([note("old.md", 100), note("new.md", 200)]);
    expect(sorted.map((n) => n.rel)).toEqual(["new.md", "old.md"]);
  });

  it("settles ties by name so the order does not shuffle between visits", () => {
    const sorted = sortDeleted([note("b.md", 100), note("a.md", 100)]);
    expect(sorted.map((n) => n.rel)).toEqual(["a.md", "b.md"]);
  });

  it("leaves the caller's array alone", () => {
    const given = [note("b.md", 1), note("a.md", 2)];
    sortDeleted(given);
    expect(given[0].rel).toBe("b.md");
  });
});

describe("formatDeletedAt", () => {
  const now = new Date(2026, 7, 7);

  it("says when it left", () => {
    expect(formatDeletedAt(new Date(2026, 7, 5).getTime() / 1000, now)).toBe("deleted Aug 5");
  });

  it("keeps the year for earlier years", () => {
    expect(formatDeletedAt(new Date(2025, 11, 24).getTime() / 1000, now)).toContain("2025");
  });

  it("admits when nothing recorded the moment", () => {
    // A note removed outside the application has no deletion time anywhere. Showing the epoch
    // would be a confident wrong answer.
    expect(formatDeletedAt(0, now)).toBe("date unknown");
  });
});

describe("restoredMessage", () => {
  it("says which state came back", () => {
    expect(restoredMessage("a.md", { rel: "a.md", asDeleted: true })).toBe(
      "Restored a. It came back as it was when you deleted it.",
    );
    expect(restoredMessage("a.md", { rel: "a.md", asDeleted: false })).toBe(
      "Restored a. It came back from its last version.",
    );
  });

  it("says so when the name had to move aside", () => {
    // Overwriting is never the answer, so the name changing has to be told rather than found.
    const message = restoredMessage("a.md", { rel: "a (1).md", asDeleted: true });
    expect(message).toContain("Restored as a (1)");
    expect(message).toContain("already there");
  });
});

describe("standing text", () => {
  it("says there is no way to remove anything for good", () => {
    // Better said than discovered by hunting for a button that is not there.
    expect(standingNote()).toContain("Nothing is removed from here");
  });

  it("says plainly when the list is empty", () => {
    expect(emptyMessage()).toBe("Nothing has been deleted.");
  });
});
