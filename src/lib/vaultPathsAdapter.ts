// Transitional: lets `ipc.ts` send vault-relative paths while the stores still hold absolute ones.
// It only ever sees paths the backend handed out for this vault. Removed once the stores carry
// relative paths themselves.

import { pathInside, samePath } from "./vaultPaths.helpers";

const trimmed = (root: string) => root.replace(/[\\/]+$/, "");

/** The vault-relative (`/`-separated) path of `abs`, a path inside the vault at `root`. */
export function relIn(root: string, abs: string): string {
  const base = trimmed(root);
  if (!pathInside(abs, base) || abs.length <= base.length + 1) {
    throw new Error(`path is outside the vault: ${abs}`);
  }
  return abs.slice(base.length + 1).replace(/\\/g, "/");
}

/** Like {@link relIn} for a folder something is made in or moved to: the vault itself is `""`. */
export function folderRelIn(root: string, abs: string): string {
  return samePath(abs, root) ? "" : relIn(root, abs);
}

/**
 * The absolute path of `rel` in the vault at `root`, spelled exactly as the backend spells the
 * paths it hands out — the stores compare them as strings. On Windows (the one platform the app
 * ships for) that is the vault path as given, then each name after a `\`.
 */
export function absOut(root: string, rel: string): string {
  const base = /[\\/]$/.test(root) ? root : `${root}\\`;
  return base + rel.split("/").join("\\");
}
