/**
 * Pure text for the dialog shown before publishing: what goes out, which notes go out as their
 * last added version, and what stays out.
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

/** Whether there is anything to publish. An empty site would replace whatever is online now. */
export function canPublish(notes: number, files: number): boolean {
  return notes + files > 0;
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

/** The sentence introducing chosen notes with newer edits, or null when there are none. */
export function unrecordedLead(unrecorded: string[]): string | null {
  if (unrecorded.length === 0) return null;
  return unrecorded.length === 1
    ? "1 note has changes you haven't added as a version. Its last added version goes out:"
    : `${unrecorded.length} notes have changes you haven't added as a version. Their last added versions go out:`;
}

/** The names of those notes, shortened. */
export function unrecordedNames(unrecorded: string[]): string {
  return shortList(unrecorded.map(displayName));
}

/** Chosen notes with no version yet: they stay out. */
export function neverRecordedLine(names: string[]): string | null {
  return names.length === 0 ? null : `Not published until you add a version: ${shortList(names.map(displayName))}`;
}

/** Files linked from published notes that are not in the last added versions. */
export function missingLine(names: string[]): string | null {
  return names.length === 0 ? null : `Linked but not in the last added versions, so left out: ${shortList(names)}`;
}

/** Notes linked from published notes that do not go out. */
export function unpublishedLinksLine(names: string[]): string | null {
  return names.length === 0
    ? null
    : `Links to notes that aren't published will lead nowhere: ${shortList(names.map(displayName))}`;
}

/** Linked files held back because the folder they sit in is left out. */
export function withheldLine(names: string[]): string | null {
  return names.length === 0
    ? null
    : `Linked but left out of the published site, so not published: ${shortList(names)}`;
}

/** What the dialog says when nothing is chosen. */
export function nothingChosenLine(): string {
  return "Nothing in this notes folder is chosen for publishing yet. You can choose notes and folders in the sidebar, or publish the whole notes folder.";
}
