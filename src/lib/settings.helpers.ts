/*
 * Pure helpers for the Settings overlay — no DOM, no localStorage, no IPC (vitest-covered).
 * Mirrors the resolveSemanticAiUi convention: the badge reflects host reality, the toggles
 * reflect the user's stored consent. The two are kept distinct so "enabled but still preparing"
 * is shown honestly.
 */
import type { ThemeMode } from "./theme.svelte";
import type { HostStatus } from "./ipc";

export type AiBadge = "ready" | "preparing" | "unavailable";

export interface AiSectionState {
  embeddingChecked: boolean;
  generationChecked: boolean;
  generationDisabled: boolean;
  badge: AiBadge;
}

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
