import { describe, expect, it } from "vitest";
import { decideStartup, LAST_VAULT_KEY } from "./startup.helpers";

describe("decideStartup", () => {
  it("restores a stored vault path", () => {
    expect(decideStartup("/home/me/Notes")).toEqual({ action: "restore", path: "/home/me/Notes" });
  });

  it("falls back to the default vault when nothing is stored", () => {
    expect(decideStartup(null)).toEqual({ action: "default" });
  });

  it("treats an empty string as no stored vault", () => {
    expect(decideStartup("")).toEqual({ action: "default" });
  });

  it("exposes the localStorage key", () => {
    expect(LAST_VAULT_KEY).toBe("textree-last-vault");
  });
});
