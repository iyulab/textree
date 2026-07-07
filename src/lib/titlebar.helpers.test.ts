import { describe, it, expect } from "vitest";
import { contextLabel } from "./titlebar.helpers";

describe("contextLabel", () => {
  it("shows the open note name in Note mode", () => {
    expect(contextLabel("note", "My Note", "my-vault")).toBe("My Note");
  });

  it("falls back to the vault name when no note is open in Note mode", () => {
    expect(contextLabel("note", "", "my-vault")).toBe("my-vault");
  });

  it("treats a whitespace-only note name as no note", () => {
    expect(contextLabel("note", "   ", "my-vault")).toBe("my-vault");
  });

  it("shows the vault name in Chat mode even with a note open", () => {
    expect(contextLabel("chat", "My Note", "my-vault")).toBe("my-vault");
  });
});
