import { describe, it, expect } from "vitest";
import { resolveSemanticAiUi } from "./semanticAiUi.helpers";

describe("resolveSemanticAiUi", () => {
  it("ready host → results", () => {
    expect(resolveSemanticAiUi(false, "ready")).toBe("ready");
    expect(resolveSemanticAiUi(true, "ready")).toBe("ready");
  });
  it("starting host → preparing (already spawned)", () => {
    expect(resolveSemanticAiUi(false, "starting")).toBe("preparing");
  });
  it("unavailable + not consented → prompt", () => {
    expect(resolveSemanticAiUi(false, "unavailable")).toBe("prompt");
  });
  it("unavailable + consented → unavailable", () => {
    expect(resolveSemanticAiUi(true, "unavailable")).toBe("unavailable");
  });
  it("null (unpolled) → preparing (neutral, no prompt flash)", () => {
    expect(resolveSemanticAiUi(false, null)).toBe("preparing");
  });
});
