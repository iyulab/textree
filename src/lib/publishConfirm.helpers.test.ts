import { describe, expect, it } from "vitest";
import {
  canPublish,
  displayName,
  missingLine,
  neverRecordedLine,
  nothingChosenLine,
  publishSummary,
  shortList,
  tooLargeLine,
  unpublishedLinksLine,
  unrecordedLead,
  unrecordedNames,
  withheldLine,
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
      "1 note has changes you haven't added as a version. Its last added version goes out:",
    );
  });

  it("introduces several in the plural", () => {
    expect(unrecordedLead(["a.md", "b.md"])).toBe(
      "2 notes have changes you haven't added as a version. Their last added versions go out:",
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

describe("the dialog under chosen publishing", () => {
  it("names chosen notes that cannot go out yet", () => {
    expect(neverRecordedLine(["fresh.md"])).toBe("Not published until you add a version: fresh");
    expect(neverRecordedLine([])).toBeNull();
  });
  it("names linked files that are left out", () => {
    expect(missingLine(["blog/gone.png"])).toBe(
      "Linked but not in the last added versions, so left out: blog/gone.png",
    );
    expect(missingLine([])).toBeNull();
  });
  it("names links that will lead nowhere", () => {
    expect(unpublishedLinksLine(["diary.md"])).toBe(
      "Links to notes that aren't published will lead nowhere: diary",
    );
    expect(unpublishedLinksLine([])).toBeNull();
  });
  it("names linked files that are held back by a choice", () => {
    expect(withheldLine(["a.png", "b.png"])).toBe(
      "Linked but left out of the published site, so not published: a.png, b.png",
    );
    expect(withheldLine([])).toBeNull();
  });
  it("names linked files too large to publish", () => {
    expect(tooLargeLine(["video.mp4"])).toBe("Too large to publish (over 50 MB), so left out: video.mp4");
    expect(tooLargeLine([])).toBeNull();
  });
  it("explains an empty choice", () => {
    expect(nothingChosenLine()).toBe(
      "Nothing in this notes folder is chosen for publishing yet. You can choose notes and folders in the sidebar, or publish the whole notes folder.",
    );
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
