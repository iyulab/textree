import { describe, it, expect } from "vitest";
import {
  computeAiSectionState,
  themeButtons,
  planEmbeddingToggle,
} from "./settings.helpers";

describe("computeAiSectionState", () => {
  it("embedding off → generation disabled and unchecked, badge unavailable", () => {
    const s = computeAiSectionState(false, false, null);
    expect(s.embeddingChecked).toBe(false);
    expect(s.generationDisabled).toBe(true);
    expect(s.generationChecked).toBe(false);
    expect(s.badge).toBe("unavailable");
  });

  it("embedding on, pre-poll (null host) → generation enabled, badge preparing", () => {
    const s = computeAiSectionState(true, false, null);
    expect(s.embeddingChecked).toBe(true);
    expect(s.generationDisabled).toBe(false);
    expect(s.badge).toBe("preparing");
  });

  it("generation checked only when both consents on", () => {
    expect(computeAiSectionState(true, true, "ready").generationChecked).toBe(true);
    // defensive: genConsent true but embedding off → not checked
    expect(computeAiSectionState(false, true, "unavailable").generationChecked).toBe(false);
  });

  it("badge maps host status: ready→ready, starting→preparing, unavailable→unavailable", () => {
    expect(computeAiSectionState(true, false, "ready").badge).toBe("ready");
    expect(computeAiSectionState(true, false, "starting").badge).toBe("preparing");
    expect(computeAiSectionState(true, false, "unavailable").badge).toBe("unavailable");
  });
});

describe("themeButtons", () => {
  it("returns auto/light/dark with the current one active", () => {
    const btns = themeButtons("dark");
    expect(btns.map((b) => b.mode)).toEqual(["auto", "light", "dark"]);
    expect(btns.map((b) => b.label)).toEqual(["Auto", "Light", "Dark"]);
    expect(btns.find((b) => b.mode === "dark")?.active).toBe(true);
    expect(btns.find((b) => b.mode === "auto")?.active).toBe(false);
  });

  it("auto is the only active one when current is auto", () => {
    const btns = themeButtons("auto");
    expect(btns.find((b) => b.mode === "auto")?.active).toBe(true);
    expect(btns.find((b) => b.mode === "light")?.active).toBe(false);
    expect(btns.find((b) => b.mode === "dark")?.active).toBe(false);
  });

  it("light is the only active one when current is light", () => {
    const btns = themeButtons("light");
    expect(btns.find((b) => b.mode === "auto")?.active).toBe(false);
    expect(btns.find((b) => b.mode === "light")?.active).toBe(true);
    expect(btns.find((b) => b.mode === "dark")?.active).toBe(false);
  });
});

describe("planEmbeddingToggle", () => {
  it("turning on keeps current generation consent and spawns", () => {
    expect(planEmbeddingToggle(true, true)).toEqual({
      nextAiConsent: true,
      nextGenConsent: true,
      host: "spawn",
    });
    expect(planEmbeddingToggle(true, false).nextGenConsent).toBe(false);
  });

  it("turning off cascades generation off and shuts down", () => {
    expect(planEmbeddingToggle(false, true)).toEqual({
      nextAiConsent: false,
      nextGenConsent: false,
      host: "shutdown",
    });
  });
});
