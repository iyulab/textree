// Transitional: lets `ipc.ts` send vault-relative paths while the stores still hold absolute ones.
// It only ever sees paths the backend handed out for this vault. Removed once the stores carry
// relative paths themselves.

import { pathInside } from "./vaultPaths.helpers";

const trimmed = (root: string) => root.replace(/[\\/]+$/, "");

/** The vault-relative (`/`-separated) path of `abs`, a path inside the vault at `root`. */
export function relIn(root: string, abs: string): string {
  const base = trimmed(root);
  if (!pathInside(abs, base) || abs.length <= base.length + 1) {
    throw new Error(`path is outside the vault: ${abs}`);
  }
  return abs.slice(base.length + 1).replace(/\\/g, "/");
}

/** The absolute path of `rel` in the vault at `root`, in the separator `root` already uses. */
export function absOut(root: string, rel: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return `${trimmed(root)}${sep}${rel.split("/").join(sep)}`;
}
