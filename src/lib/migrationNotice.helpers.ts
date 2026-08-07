/**
 * What to tell someone whose folder just stopped holding things that were never their notes.
 *
 * Said once, when it happens, and only about what actually moved. A notice for a move that did
 * not happen trains people to dismiss notices.
 */
import type { MoveOut } from "./ipc";

export const TITLE = "Two things moved out of your notes folder";

/** Whether there is anything to tell. */
export function worthShowing(moved: MoveOut): boolean {
  return moved.settings || moved.notes > 0;
}

/** The lines to show, in order. Only the ones that happened. */
export function lines(moved: MoveOut): string[] {
  const out: string[] = [];
  if (moved.settings) {
    out.push("Favourites and saved ordering now live with the app instead of inside the folder.");
  }
  if (moved.notes > 0) {
    const notes = moved.notes === 1 ? "1 note" : `${moved.notes} notes`;
    out.push(`${notes} from the trash are now under Deleted notes.`);
  }
  return out;
}

/**
 * The closing line.
 *
 * Says the part people actually worry about — that nothing was thrown away — and the part they
 * would otherwise find out by downgrading, which is that going back to an older version leaves
 * the favourites behind.
 */
export function reassurance(): string {
  return "Nothing was removed. Your folder now holds only your notes. Going back to an older version of the app would not find the favourites.";
}
