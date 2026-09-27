import { describe, expect, it } from "vitest";
import {
  canPublish,
  displayName,
  hiddenLine,
  publishSummary,
  shortList,
  unrecordedLead,
  unrecordedNames,
} from "./publishConfirm.helpers";

const web = { kind: "web" } as const;

describe("canPublish", () => {
  it("refuses an empty site, which would replace what is online", () => {
    expect(canPublish(0, 0)).toBe(false);
    expect(canPublish(1, 0)).toBe(true);
    expect(canPublish(0, 2)).toBe(true);
  });
});

describe("publishSummary", () => {
  it("counts notes and other files, and says where they go", () => {
    expect(publishSummary(12, 3, web)).toBe(
      "12 notes and 3 other files in this folder will be published to the web.",
    );
    expect(publishSummary(2, 1, { kind: "folder", path: "D:/site" })).toBe(
      "2 notes and 1 other file in this folder will be published into D:/site.",
    );
  });

  it("uses the singular for one", () => {
    expect(publishSummary(1, 0, web)).toBe("1 note in this folder will be published to the web.");
  });

  it("leaves out a count of zero", () => {
    expect(publishSummary(4, 0, web)).toBe("4 notes in this folder will be published to the web.");
    expect(publishSummary(0, 2, web)).toBe(
      "2 other files in this folder will be published to the web.",
    );
  });

  it("says so when there is nothing to publish", () => {
    expect(publishSummary(0, 0, web)).toBe(
      "Nothing in this folder can be published to the web yet.",
    );
  });
});

describe("unrecorded notes", () => {
  it("says nothing when every note matches its newest version", () => {
    expect(unrecordedLead([])).toBeNull();
  });

  it("introduces one note in the singular", () => {
    expect(unrecordedLead(["a.md"])).toBe(
      "1 note has changes you haven't added as a version. It goes out as it is now:",
    );
  });

  it("introduces several in the plural", () => {
    expect(unrecordedLead(["a.md", "b.md"])).toBe(
      "2 notes have changes you haven't added as a version. They go out as they are now:",
    );
  });

  it("names notes by path without the extension", () => {
    expect(unrecordedNames(["a.md", "sub/Idea.MD"])).toBe("a, sub/Idea");
  });

  it("names at most five, then how many more", () => {
    const seven = ["1", "2", "3", "4", "5", "6", "7"].map((n) => `${n}.md`);
    expect(unrecordedNames(seven)).toBe("1, 2, 3, 4, 5 and 2 more");
  });
});

describe("hiddenLine", () => {
  it("says nothing when nothing is hidden", () => {
    expect(hiddenLine([])).toBeNull();
  });

  it("lists the hidden files", () => {
    expect(hiddenLine([".env", ".gitignore"])).toBe("Hidden files are left out: .env, .gitignore");
  });

  it("caps the list", () => {
    const many = [".a", ".b", ".c", ".d", ".e", ".f"];
    expect(hiddenLine(many)).toBe("Hidden files are left out: .a, .b, .c, .d, .e and 1 more");
  });
});

describe("list helpers", () => {
  it("keeps a non-note name as it is", () => {
    expect(displayName("image.png")).toBe("image.png");
  });

  it("shows a short list whole", () => {
    expect(shortList(["x", "y"])).toBe("x, y");
  });
});
