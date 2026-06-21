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
