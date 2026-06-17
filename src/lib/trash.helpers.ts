import type { TrashItem } from "./ipc";

/** Sorts by deletion time, newest first; unknown (deletedAt 0) sinks to the bottom. Pure (copies). */
export function sortTrash(items: TrashItem[]): TrashItem[] {
  return [...items].sort((a, b) => b.deletedAt - a.deletedAt);
}

/** Formats epoch seconds as local `YYYY-MM-DD HH:mm`; 0 → "unknown". */
export function formatDeletedAt(epochSecs: number): string {
  if (epochSecs === 0) return "unknown";
  const d = new Date(epochSecs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Human label for an item's origin; unknown-origin (no manifest entry) reads "unknown origin". */
export function originLabel(item: TrashItem): string {
  if (item.deletedAt === 0 && item.originalRel === item.trashName) return "unknown origin";
  return item.originalRel;
}
