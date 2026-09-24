import { describe, expect, it } from "vitest";
import { decideChangeAction, decideReopen, decideRescanAction } from "./sync.helpers";

describe("decideRescanAction", () => {
  it("leaves a note alone when disk matches what the user sees (clean)", () => {
    expect(decideRescanAction("same", "same", false, "same")).toBe("none");
  });

  it("leaves a note alone when disk matches what the user sees (dirty, autosave already flushed)", () => {
    expect(decideRescanAction("typed text", "typed text", true, "older")).toBe("none");
  });

  it("silently reloads a clean note whose disk content diverged", () => {
    expect(decideRescanAction("external edit", "loaded text", false, "loaded text")).toBe("reload");
  });

  it("raises the conflict banner when disk diverged and there are unsaved edits", () => {
    expect(decideRescanAction("disk text", "my unsaved text", true, "what I last loaded")).toBe("conflict");
  });

  it("does not ask about the user's own typing when disk still holds what was last synced", () => {
    // Rescan while typing (e.g. adding the first version creates the repository's HEAD record):
    // nothing changed on disk, only the editor moved ahead of it.
    expect(decideRescanAction("saved text", "saved text + typing", true, "saved text")).toBe("none");
  });
});

describe("decideChangeAction", () => {
  it("reloads a clean note that changed on disk", () => {
    expect(decideChangeAction("changed elsewhere", "on screen", false, "on screen")).toBe("reload");
  });

  it("leaves a clean note alone when disk already holds what the user sees (the app moved or renamed it)", () => {
    expect(decideChangeAction("same", "same", false, "same")).toBe("none");
  });

  it("raises a conflict when disk moved away from the last sync while there are unsaved edits", () => {
    expect(decideChangeAction("edited elsewhere", "typing", true, "last saved")).toBe("conflict");
  });

  it("ignores a change event whose content is still what was last synced", () => {
    expect(decideChangeAction("last saved", "typing", true, "last saved")).toBe("none");
  });
});

describe("decideReopen", () => {
  it("keeps the editor while the note's own edits are still being written, even though disk differs", () => {
    expect(decideReopen(true, "older on disk", "newer on screen")).toBe("keep");
  });

  it("rebuilds from disk when disk no longer holds what the screen shows (a version was restored)", () => {
    expect(decideReopen(false, "restored", "before restoring")).toBe("rebuild");
  });

  it("reuses the editor when disk holds what is on screen", () => {
    expect(decideReopen(false, "same", "same")).toBe("reuse");
  });
});
