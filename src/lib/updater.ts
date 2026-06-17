import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateInfo {
  version: string;
  notes: string;
  handle: Update;
}

/**
 * Checks for an available update. Returns null when up to date or when the updater is
 * unavailable (e.g. dev build, non-Windows). Never throws to the caller — startup must
 * proceed even if the update check fails (graceful degradation).
 */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  try {
    const update = await check();
    if (!update) return null;
    return { version: update.version, notes: update.body ?? "", handle: update };
  } catch {
    return null;
  }
}

/**
 * Downloads and installs the given update, then relaunches. Only called after explicit
 * user consent (button click) — never silently (content-safety guard).
 */
export async function applyUpdate(info: UpdateInfo): Promise<void> {
  await info.handle.downloadAndInstall();
  await relaunch();
}
