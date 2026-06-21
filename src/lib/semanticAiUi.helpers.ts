import type { HostStatus } from "./ipc";

/** What the semantic palette should surface, given device consent and host status. */
export type SemanticAiUi = "prompt" | "preparing" | "ready" | "unavailable";

/**
 * Pure mapping (no DOM/localStorage). `consent` = device-local AI consent flag; `host` = the last
 * polled host status (null before the first poll).
 * - ready → results
 * - starting → preparing (host already spawned; first run is downloading the model)
 * - unavailable → prompt to enable if not consented, else a calm unavailable row
 * - null → preparing (neutral; avoids flashing the prompt before the status is known)
 */
export function resolveSemanticAiUi(consent: boolean, host: HostStatus | null): SemanticAiUi {
  if (host === "ready") return "ready";
  if (host === "starting") return "preparing";
  if (host === "unavailable") return consent ? "unavailable" : "prompt";
  return "preparing";
}
