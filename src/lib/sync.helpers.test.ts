import { describe, expect, it } from "vitest";
import { decideRescanAction } from "./sync.helpers";

describe("decideRescanAction", () => {
  it("leaves a note alone when disk matches what the user sees (clean)", () => {
    expect(decideRescanAction("same", "same", false)).toBe("none");
  });

  it("leaves a note alone when disk matches what the user sees (dirty, autosave already flushed)", () => {
    expect(decideRescanAction("typed text", "typed text", true)).toBe("none");
  });

  it("silently reloads a clean note whose disk content diverged", () => {
    expect(decideRescanAction("external edit", "loaded text", false)).toBe("reload");
  });

  it("raises the conflict banner when disk diverged and there are unsaved edits", () => {
    expect(decideRescanAction("disk text", "my unsaved text", true)).toBe("conflict");
  });
});
