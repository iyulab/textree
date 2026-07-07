// Device-local BYO (bring-your-own OpenAI-compatible endpoint) config — non-secret fields only
// (preset/baseUrl/model). Same localStorage convention as aiConsent.ts. The API key is never
// stored here — it lives in the OS Credential Manager via the set/clear/has_byo_api_key IPC
// commands (ipc.ts), and this module never sees its plaintext value.
export type ByoPreset =
  | "ollama" | "gpustack" | "openai" | "anthropic" | "gemini" | "grok" | "custom";

export interface ByoConfig {
  preset: ByoPreset;
  baseUrl: string;
  model: string;
}

const ENABLED_KEY = "byo-enabled";
const PRESET_KEY = "byo-preset";
const BASE_URL_KEY = "byo-base-url";
const MODEL_KEY = "byo-model";

function isByoPreset(value: string | null): value is ByoPreset {
  return (
    value === "ollama" || value === "gpustack" || value === "openai" ||
    value === "anthropic" || value === "gemini" || value === "grok" ||
    value === "custom"
  );
}

export function getByoConfig(): ByoConfig | null {
  try {
    if (localStorage.getItem(ENABLED_KEY) !== "true") return null;
    const preset = localStorage.getItem(PRESET_KEY);
    if (!isByoPreset(preset)) return null;
    return {
      preset,
      baseUrl: localStorage.getItem(BASE_URL_KEY) ?? "",
      model: localStorage.getItem(MODEL_KEY) ?? "",
    };
  } catch {
    return null;
  }
}

export function setByoConfig(config: ByoConfig): void {
  try {
    localStorage.setItem(ENABLED_KEY, "true");
    localStorage.setItem(PRESET_KEY, config.preset);
    localStorage.setItem(BASE_URL_KEY, config.baseUrl);
    localStorage.setItem(MODEL_KEY, config.model);
  } catch {
    /* private mode / no storage — BYO simply won't persist across restarts */
  }
}

export function clearByoConfig(): void {
  try {
    localStorage.setItem(ENABLED_KEY, "false");
  } catch {
    /* no-op */
  }
}
