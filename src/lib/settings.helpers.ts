/*
 * Pure helpers for the Settings overlay — no DOM, no localStorage, no IPC (vitest-covered).
 * Mirrors the resolveSemanticAiUi convention: the badge reflects host reality, the toggles
 * reflect the user's stored consent. The two are kept distinct so "enabled but still preparing"
 * is shown honestly.
 */
import type { ThemeMode } from "./theme.svelte";
import type { HostStatus } from "./ipc";
import type { ByoPreset } from "./byoConfig";

export type AiBadge = "ready" | "preparing" | "unavailable";

export interface AiSectionState {
  embeddingChecked: boolean;
  generationChecked: boolean;
  generationDisabled: boolean;
  badge: AiBadge;
}

// `host === null` is the pre-poll state: refreshHost() has not resolved yet (the modal
// just opened). Show "preparing" optimistically when the user has consented — we expect
// the host to come up — and "unavailable" otherwise, so the badge never overpromises.
function aiBadge(aiConsent: boolean, host: HostStatus | null): AiBadge {
  if (host === "ready") return "ready";
  if (host === "starting") return "preparing";
  if (host === null) return aiConsent ? "preparing" : "unavailable";
  return "unavailable"; // host === "unavailable"
}

/** Display state for the Local AI section. Generation requires embedding (host must exist). */
export function computeAiSectionState(
  aiConsent: boolean,
  genConsent: boolean,
  host: HostStatus | null,
): AiSectionState {
  return {
    embeddingChecked: aiConsent,
    generationChecked: aiConsent && genConsent,
    generationDisabled: !aiConsent,
    badge: aiBadge(aiConsent, host),
  };
}

export interface ThemeButton {
  mode: ThemeMode;
  label: string;
  active: boolean;
}

const THEME_LABELS: Record<ThemeMode, string> = { auto: "Auto", light: "Light", dark: "Dark" };

export function themeButtons(current: ThemeMode): ThemeButton[] {
  return (["auto", "light", "dark"] as ThemeMode[]).map((mode) => ({
    mode,
    label: THEME_LABELS[mode],
    active: mode === current,
  }));
}

export interface EmbeddingTogglePlan {
  nextAiConsent: boolean;
  nextGenConsent: boolean;
  host: "spawn" | "shutdown";
}

/**
 * Plan for flipping the embedding/search consent. Turning OFF cascades generation OFF
 * (generation needs the host) and shuts the host down; turning ON keeps generation consent
 * as-is and spawns the host.
 */
export function planEmbeddingToggle(next: boolean, currentGenConsent: boolean): EmbeddingTogglePlan {
  if (next) return { nextAiConsent: true, nextGenConsent: currentGenConsent, host: "spawn" };
  return { nextAiConsent: false, nextGenConsent: false, host: "shutdown" };
}

const PRESET_DEFAULT_BASE_URL: Record<ByoPreset, string> = {
  ollama: "http://localhost:11434",
  gpustack: "http://localhost:8080",
  openai: "https://api.openai.com/v1",
  anthropic: "",
  gemini: "https://generativelanguage.googleapis.com/v1beta/openai",
  grok: "https://api.x.ai/v1",
  custom: "",
};

/** Prefill values for the preset segmented control in Settings ▸Advanced. */
export function presetDefaults(preset: ByoPreset): { baseUrl: string } {
  return { baseUrl: PRESET_DEFAULT_BASE_URL[preset] };
}

/** Minimal client-side sanity check before enabling the "Test connection"/"Save" buttons. */
export function isValidByoUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

/** Anthropic's SDK supplies its own base URL, so a blank value is valid for that preset only.
 * Every other preset needs a concrete http(s) endpoint. */
export function isValidByoUrlForPreset(preset: ByoPreset, url: string): boolean {
  if (preset === "anthropic" && url.trim() === "") return true;
  return isValidByoUrl(url);
}

/** Gates the Save button: a blank model has no real "provider default" to fall back on — the
 * host would send the literal placeholder string "default" as the model name, which fails on
 * the very first chat request against a real server. Required, not just recommended. */
export function isValidByoModel(model: string): boolean {
  return model.trim().length > 0;
}

/** Badge text for "currently running on: ...". Empty string mirrors HostHandle's default
 * (Rust's #[derive(Default)] gives Mutex<String> "" rather than "local" — both map here). */
export function byoProviderBadge(activeProvider: string): string {
  if (activeProvider === "" || activeProvider === "local") return "Local model";
  const labels: Record<string, string> = {
    ollama: "Ollama", gpustack: "GPUStack", openai: "OpenAI",
    anthropic: "Anthropic", gemini: "Gemini", grok: "Grok",
  };
  return `Custom server (${labels[activeProvider] ?? activeProvider})`;
}
