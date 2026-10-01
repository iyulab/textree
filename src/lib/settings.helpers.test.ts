import { describe, it, expect } from "vitest";
import { BYO_PRESET_IDS } from "./byoConfig";
import presetFile from "./byo-presets.json";
import {
  computeAiSectionState,
  themeButtons,
  planEmbeddingToggle,
  presetDefaults,
  isValidByoUrl,
  isValidByoUrlForPreset,
  isValidByoModel,
  byoProviderBadge,
  byoTestLabel,
  byoKeyRequired,
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

describe("presetDefaults", () => {
  it("returns Ollama's default base URL", () => {
    expect(presetDefaults("ollama")).toEqual({ baseUrl: "http://localhost:11434" });
  });
  it("returns GPUStack's default base URL", () => {
    expect(presetDefaults("gpustack")).toEqual({ baseUrl: "http://localhost:8080" });
  });
  it("returns an empty base URL for custom", () => {
    expect(presetDefaults("custom")).toEqual({ baseUrl: "" });
  });
});

describe("presetDefaults frontier baseUrls", () => {
  it("prefills every hosted provider with its own address", () => {
    expect(presetDefaults("openai").baseUrl).toBe("https://api.openai.com/v1");
    expect(presetDefaults("grok").baseUrl).toBe("https://api.x.ai/v1");
    expect(presetDefaults("anthropic").baseUrl).toBe("https://api.anthropic.com");
    expect(presetDefaults("gemini").baseUrl).toBe("https://generativelanguage.googleapis.com");
  });
});

describe("isValidByoUrlForPreset", () => {
  it("accepts a blank baseUrl for every preset with a default address", () => {
    for (const p of ["ollama", "gpustack", "openai", "anthropic", "gemini", "grok"] as const) {
      expect(isValidByoUrlForPreset(p, "")).toBe(true);
    }
  });
  it("rejects a blank baseUrl for custom, which has no default", () => {
    expect(isValidByoUrlForPreset("custom", "")).toBe(false);
  });
  it("accepts a valid http(s) baseUrl for any preset", () => {
    expect(isValidByoUrlForPreset("openai", "https://api.openai.com/v1")).toBe(true);
    expect(isValidByoUrlForPreset("custom", "http://lan:8080/v1")).toBe(true);
  });
  it("rejects a malformed baseUrl even where blank would do", () => {
    expect(isValidByoUrlForPreset("openai", "not a url")).toBe(false);
  });
});

describe("isValidByoUrl", () => {
  it("accepts http/https URLs", () => {
    expect(isValidByoUrl("http://localhost:11434")).toBe(true);
    expect(isValidByoUrl("https://gpustack.internal:8080")).toBe(true);
  });
  it("rejects empty or malformed input", () => {
    expect(isValidByoUrl("")).toBe(false);
    expect(isValidByoUrl("not a url")).toBe(false);
    expect(isValidByoUrl("ftp://localhost")).toBe(false);
  });
});

describe("isValidByoModel", () => {
  it("rejects empty or whitespace-only input", () => {
    expect(isValidByoModel("")).toBe(false);
    expect(isValidByoModel("   ")).toBe(false);
  });
  it("accepts a non-empty model name", () => {
    expect(isValidByoModel("llama3.1")).toBe(true);
    expect(isValidByoModel("  llama3.1  ")).toBe(true);
  });
});

describe("byoProviderBadge", () => {
  it("shows the local label for empty/local activeProvider", () => {
    expect(byoProviderBadge("")).toBe("Local model");
    expect(byoProviderBadge("local")).toBe("Local model");
  });
  it("shows the preset name for a BYO preset", () => {
    expect(byoProviderBadge("ollama")).toBe("Custom server (Ollama)");
  });
});

describe("byoKeyRequired", () => {
  it("asks for a key for hosted providers", () => {
    for (const p of ["openai", "anthropic", "gemini", "grok"] as const) expect(byoKeyRequired(p)).toBe(true);
  });
  it("leaves the key optional for local and other OpenAI-compatible servers", () => {
    for (const p of ["ollama", "gpustack", "custom"] as const) expect(byoKeyRequired(p)).toBe(false);
  });
});

describe("byoTestLabel", () => {
  it("says it is checking while the check runs", () => {
    expect(byoTestLabel("checking")).toBe("Checking…");
  });
  it("names how many models the server offers", () => {
    expect(byoTestLabel({ ok: true, modelCount: 141 })).toBe("✓ Connected — 141 models available");
    expect(byoTestLabel({ ok: true, modelCount: 1 })).toBe("✓ Connected — 1 model available");
  });
  it("carries the reason of a failed check", () => {
    expect(byoTestLabel({ ok: false, message: "The server didn't accept the API key." }))
      .toBe("✗ Connection failed: The server didn't accept the API key.");
  });
});

describe("byo-presets.json", () => {
  it("lists exactly the presets the app knows, in the order Settings offers them", () => {
    expect(Object.keys(presetFile)).toEqual(["ollama", "gpustack", "openai", "anthropic", "gemini", "grok", "custom"]);
    expect(BYO_PRESET_IDS).toEqual(Object.keys(presetFile));
  });
});
