import { describe, expect, it } from "vitest";
import { backupStatus, NOT_BACKED_UP } from "./backupStatus.helpers";

describe("backupStatus", () => {
  it("warns while notes have no copy elsewhere", () => {
    expect(backupStatus(false)).toEqual(NOT_BACKED_UP);
  });

  it("shows nothing once notes are backed up", () => {
    expect(backupStatus(true)).toBeNull();
  });

  it("says what is at risk, not just that something is missing", () => {
    expect(NOT_BACKED_UP.label).toBe("Not backed up");
    expect(NOT_BACKED_UP.tooltip).toMatch(/only on this computer/);
    expect(NOT_BACKED_UP.tooltip).toMatch(/versions/);
  });
});
