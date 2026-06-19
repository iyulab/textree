import { describe, expect, it } from "vitest";
import { friendlyError } from "./friendlyError.helpers";

describe("friendlyError", () => {
  it("maps an OS permission error to a helpful summary and preserves the raw text", () => {
    const r = friendlyError("Permission denied (os error 13)");
    expect(r.summary).toMatch(/permission/i);
    expect(r.raw).toBe("Permission denied (os error 13)");
    // summary differs from raw, so the UI knows to show the raw as a diagnostic aside
    expect(r.summary).not.toBe(r.raw);
  });

  it("maps a missing-path OS error (os error 2) to a not-found summary", () => {
    const r = friendlyError("No such file or directory (os error 2)");
    expect(r.summary).toMatch(/could not be found|moved or deleted/i);
    expect(r.raw).toBe("No such file or directory (os error 2)");
  });

  it("maps a disk-full OS error (os error 28) to a disk-space summary", () => {
    const r = friendlyError("No space left on device (os error 28)");
    expect(r.summary).toMatch(/disk space/i);
  });

  it("maps a name-collision domain error to a friendly summary", () => {
    const r = friendlyError("an item with the same name already exists");
    expect(r.summary).toMatch(/already exists/i);
  });

  it("maps a canopy publish failure to a publishing-tool summary", () => {
    const r = friendlyError("canopy failed: oniguruma not found");
    expect(r.summary).toMatch(/publish/i);
    // the underlying detail is kept verbatim for diagnosis
    expect(r.raw).toContain("oniguruma not found");
  });

  it("maps an out-of-vault publish boundary error to actionable guidance", () => {
    const r = friendlyError("the output directory must be outside the vault");
    expect(r.summary).toMatch(/outside your vault/i);
  });

  it("normalizes a thrown Error object via its message/string form", () => {
    const r = friendlyError(new Error("Permission denied (os error 13)"));
    expect(r.summary).toMatch(/permission/i);
    expect(r.raw).toContain("Permission denied (os error 13)");
  });

  it("falls back to the raw text when the error is unrecognized (no silent loss)", () => {
    const r = friendlyError("totally novel backend failure xyz");
    // unknown → summary IS the raw text; nothing is hidden or swallowed
    expect(r.summary).toBe("totally novel backend failure xyz");
    expect(r.raw).toBe("totally novel backend failure xyz");
  });

  it("handles empty/whitespace input without producing an empty message", () => {
    const r = friendlyError("");
    expect(r.summary.length).toBeGreaterThan(0);
  });
});
