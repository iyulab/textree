import { describe, it, expect, beforeEach } from "vitest";
import { getByoConfig, setByoConfig, clearByoConfig } from "./byoConfig";

// Vitest runs this file under Node ("environment: node" in vitest.config.ts), which has no
// browser `localStorage` global. Polyfill the small subset byoConfig.ts relies on so this test
// can exercise real get/set/removeItem calls without pulling in jsdom/happy-dom just for this.
if (typeof globalThis.localStorage === "undefined") {
  const store = new Map<string, string>();
  globalThis.localStorage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key: string) => (store.has(key) ? store.get(key)! : null),
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    removeItem: (key: string) => {
      store.delete(key);
    },
    setItem: (key: string, value: string) => {
      store.set(key, String(value));
    },
  };
}

describe("byoConfig", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("returns null when nothing has been configured", () => {
    expect(getByoConfig()).toBeNull();
  });

  it("round-trips a saved config", () => {
    setByoConfig({ preset: "ollama", baseUrl: "http://localhost:11434", model: "llama3" });
    expect(getByoConfig()).toEqual({
      preset: "ollama",
      baseUrl: "http://localhost:11434",
      model: "llama3",
    });
  });

  it("returns null after clearByoConfig", () => {
    setByoConfig({ preset: "gpustack", baseUrl: "http://localhost:8080", model: "" });
    clearByoConfig();
    expect(getByoConfig()).toBeNull();
  });

  it("returns null when the stored preset is not a recognized value (corrupted storage)", () => {
    localStorage.setItem("byo-enabled", "true");
    localStorage.setItem("byo-preset", "not-a-real-preset");
    expect(getByoConfig()).toBeNull();
  });
});

describe("ByoPreset frontier presets", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  for (const preset of ["openai", "anthropic", "gemini", "grok"] as const) {
    it(`round-trips the ${preset} preset`, () => {
      setByoConfig({ preset, baseUrl: "https://example.test/v1", model: "m" });
      expect(getByoConfig()).toEqual({
        preset, baseUrl: "https://example.test/v1", model: "m",
      });
    });
  }
});
