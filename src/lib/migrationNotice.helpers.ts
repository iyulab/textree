/**
 * What to tell someone whose folder just stopped holding things that were never their notes.
 *
 * Said when it happens, and only about what actually happened. A notice for a move that did not
 * happen trains people to dismiss notices; a notice that calls the folder clean while something
 * stayed behind is worse, because it is believed.
 */
import type { MoveOut } from "./ipc";

/** How many names of what stayed behind are spelled out before the rest is counted. */
const NAMED = 3;

/** Whether there is anything to tell. */
export function worthShowing(moved: MoveOut): boolean {
  return moved.settings || moved.notes > 0 || moved.files > 0 || moved.leftBehind.length > 0;
}

/** The heading — it counts what moved, so it never promises more than the lines below say. */
export function title(moved: MoveOut): string {
  const kinds = Number(moved.settings) + Number(moved.notes + moved.files > 0);
  if (kinds === 2) return "Two things moved out of your notes folder";
  if (kinds === 1) return "Moved out of your notes folder";
  return "Some files stayed in your notes folder";
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The lines to show, in order. Only the ones that happened. */
export function lines(moved: MoveOut): string[] {
  const out: string[] = [];
  if (moved.settings) {
    out.push("Favourites, ordering and saved views now live with the app instead of inside the folder.");
  }
  if (moved.notes > 0 || moved.files > 0) {
    const parts: string[] = [];
    if (moved.notes > 0) parts.push(plural(moved.notes, "note", "notes"));
    if (moved.files > 0) {
      parts.push(plural(moved.files, moved.notes > 0 ? "other file" : "file", moved.notes > 0 ? "other files" : "files"));
    }
    const verb = moved.notes + moved.files === 1 ? "is" : "are";
    out.push(`${parts.join(" and ")} from the trash ${verb} now under Deleted notes.`);
  }
  if (moved.leftBehind.length > 0) {
    const named = moved.leftBehind.slice(0, NAMED).join(", ");
    const rest = moved.leftBehind.length - NAMED;
    const more = rest > 0 ? `, and ${plural(rest, "more", "more")}` : "";
    out.push(`These could not be moved and are still in the folder, untouched: ${named}${more}.`);
  }
  return out;
}

/**
 * The closing line.
 *
 * Says the part people actually worry about — that nothing was thrown away. Says the folder is
 * clean only when it is. And, when favourites moved, the part they would otherwise find out by
 * downgrading: an older version of the app looks for them where they no longer are.
 */
export function reassurance(moved: MoveOut): string {
  const said = ["Nothing was removed."];
  if (moved.leftBehind.length === 0) said.push("Your folder now holds only your notes.");
  if (moved.settings) said.push("Going back to an older version of the app would not find the favourites.");
  return said.join(" ");
}
