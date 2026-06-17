/** localStorage key holding the absolute path of the last opened vault (device-bound). */
export const LAST_VAULT_KEY = "textree-last-vault";

export type StartupPlan = { action: "restore"; path: string } | { action: "default" };

/**
 * Decides what to open at startup: a stored vault is restored; otherwise (first run) the
 * default vault is created/opened. Pure — the caller performs the localStorage/IPC effects.
 */
export function decideStartup(lastVault: string | null): StartupPlan {
  if (lastVault && lastVault.length > 0) return { action: "restore", path: lastVault };
  return { action: "default" };
}
