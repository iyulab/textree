// Pure helper for the title bar's center button label. Kept runes-free so vitest can import it
// (pure ↔ runes separation). Note mode shows the open note; everything else shows the vault.
export function contextLabel(
  mode: "note" | "chat",
  noteName: string,
  vaultName: string,
): string {
  if (mode === "note" && noteName.trim().length > 0) return noteName;
  return vaultName;
}
