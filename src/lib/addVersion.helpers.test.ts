import { describe, it, expect } from "vitest";
import {
  defaultVersionName,
  noteName,
  outcomeMessage,
  scopeSummary,
  versionName,
} from "./addVersion.helpers";

describe("noteName", () => {
  it("reads the name the way the screen shows it", () => {
    expect(noteName("C:\\vault\\sub\\Plan draft.md")).toBe("Plan draft");
    expect(noteName("sub/Plan draft.md")).toBe("Plan draft");
    expect(noteName("Plan draft")).toBe("Plan draft");
  });

  it("only takes off a markdown extension, not any dot", () => {
    expect(noteName("release.notes.md")).toBe("release.notes");
    expect(noteName("archive.tar")).toBe("archive.tar");
  });

  it("keeps names that are not written in the Latin alphabet", () => {
    // A note title is whatever its author wrote. Reducing this fixture to ASCII would leave the
    // case it exists to cover untested.
    // A name outside the Latin alphabet, so that the slice is by character and not by byte.
    // forbidden-tokens: allow non-latin - the multi-byte name is what this case is about
    expect(noteName("sub/메모.md")).toBe("메모");
  });
});

describe("defaultVersionName", () => {
  it("names one note by its own name", () => {
    expect(defaultVersionName(["sub/a.md"])).toBe("a");
  });

  it("names several by the first and how many others there were", () => {
    expect(defaultVersionName(["a.md", "b.md"])).toBe("a and 1 more");
    expect(defaultVersionName(["a.md", "b.md", "c.md"])).toBe("a and 2 more");
  });

  it("has nothing to say about nothing", () => {
    expect(defaultVersionName([])).toBe("");
  });
});

describe("versionName", () => {
  it("uses what was typed", () => {
    expect(versionName("  Rewrote the introduction  ", ["a.md"])).toBe(
      "Rewrote the introduction",
    );
  });

  it("falls back when nothing was typed, including only spaces", () => {
    expect(versionName("", ["a.md"])).toBe("a");
    expect(versionName("   \n ", ["a.md"])).toBe("a");
  });
});

describe("scopeSummary", () => {
  it("says how far the version reaches", () => {
    expect(scopeSummary(["a.md"])).toBe("This note only");
    expect(scopeSummary(["a.md", "b.md"])).toBe("2 notes");
    expect(scopeSummary([])).toBe("Nothing selected");
  });
});

describe("outcomeMessage", () => {
  it("says what was added", () => {
    expect(outcomeMessage("abc123", ["sub/a.md"])).toBe("Added a version of a.");
    expect(outcomeMessage("abc123", ["a.md", "b.md"])).toBe("Added a version of 2 notes.");
  });

  it("does not claim a version was added when none was", () => {
    // The backend answers with no revision when the notes already hold the recorded state.
    // Reporting success here would point at a version the history does not list.
    expect(outcomeMessage(null, ["a.md"])).toBe(
      "Nothing to add — this note hasn't changed since its last version.",
    );
    expect(outcomeMessage(null, ["a.md", "b.md"])).toBe(
      "Nothing to add — these notes haven't changed since their last version.",
    );
  });
});
