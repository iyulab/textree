// Device-local AI consent flag (localStorage — the same home as width/theme/last-vault, per the
// project's device-local state convention). A thin wrapper; not unit-tested.
const KEY = "ai-consent";

export function getAiConsent(): boolean {
  try {
    return localStorage.getItem(KEY) === "true";
  } catch {
    return false;
  }
}

export function setAiConsent(value: boolean): void {
  try {
    localStorage.setItem(KEY, value ? "true" : "false");
  } catch {
    /* private mode / no storage — treat as not consented */
  }
}

// Generation consent — separate from embedding/search consent (ai-consent).
// Enabling Q&A implies enabling local AI too (setAiConsent must also be called).
const GEN_KEY = "ai-generation-consent";

export function getGenerationConsent(): boolean {
  try {
    return localStorage.getItem(GEN_KEY) === "true";
  } catch {
    return false;
  }
}

export function setGenerationConsent(value: boolean): void {
  try {
    localStorage.setItem(GEN_KEY, String(value));
  } catch {
    /* private mode — treat as not consented */
  }
}
