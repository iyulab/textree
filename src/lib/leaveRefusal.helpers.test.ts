import { describe, expect, it } from "vitest";
import { leaveRefusal } from "./leaveRefusal.helpers";

describe("leaveRefusal", () => {
  it("says the save failed only when it did", () => {
    expect(leaveRefusal("Move", "failed")).toBe("Move canceled — could not save your unsaved edits.");
  });

  it("says the edits are still being saved when they are, without calling it a failure", () => {
    const text = leaveRefusal("Delete", "busy");
    expect(text).toBe("Delete canceled — your last edits are still being saved. Try again in a moment.");
    expect(text).not.toMatch(/could not/);
  });
});
