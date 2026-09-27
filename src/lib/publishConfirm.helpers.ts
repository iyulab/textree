/**
 * Pure text for the dialog shown before publishing: what goes out, what goes out as it is now
 * rather than as a version, and what is left out.
 */

/** Where the site goes. */
export type PublishDestination = { kind: "web" } | { kind: "folder"; path: string };

/** How many names a list shows before summing up the rest. */
export const LIST_LIMIT = 5;

function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

function where(destination: PublishDestination): string {
  return destination.kind === "web" ? "to the web" : `into ${destination.path}`;
}

/** One line: how much goes out, and where. */
export function publishSummary(
  notes: number,
  files: number,
  destination: PublishDestination,
): string {
  if (notes === 0 && files === 0) {
    return `Nothing in this folder can be published ${where(destination)} yet.`;
  }
  const parts: string[] = [];
  if (notes > 0) parts.push(count(notes, "note", "notes"));
  if (files > 0) parts.push(count(files, "other file", "other files"));
  return `${parts.join(" and ")} in this folder will be published ${where(destination)}.`;
}

/** A note's vault-relative path as it reads on screen: without the markdown extension. */
export function displayName(rel: string): string {
  return rel.toLowerCase().endsWith(".md") ? rel.slice(0, -3) : rel;
}

/** Up to `LIST_LIMIT` names, then how many more. */
export function shortList(names: string[]): string {
  const shown = names.slice(0, LIST_LIMIT).join(", ");
  const rest = names.length - LIST_LIMIT;
  return rest > 0 ? `${shown} and ${rest} more` : shown;
}

/** The sentence introducing the notes that go out without a version, or null when there are none. */
export function unrecordedLead(unrecorded: string[]): string | null {
  if (unrecorded.length === 0) return null;
  return unrecorded.length === 1
    ? "1 note has changes you haven't added as a version. It goes out as it is now:"
    : `${unrecorded.length} notes have changes you haven't added as a version. They go out as they are now:`;
}

/** The names of those notes, shortened. */
export function unrecordedNames(unrecorded: string[]): string {
  return shortList(unrecorded.map(displayName));
}

/** What is left out, or null when nothing is. */
export function hiddenLine(hidden: string[]): string | null {
  if (hidden.length === 0) return null;
  return `Hidden files are left out: ${shortList(hidden)}`;
}
