/**
 * Pure text for the dialog that records a version.
 *
 * Naming a version is optional. Requiring one would make the fast path — record what I have
 * right now — slower than the thing it protects against, and a person who cannot think of a
 * name still wants the version.
 */

/** Last component of a path, separator-tolerant. */
function lastSegment(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

/** A note's name as it reads on screen: the file name without the markdown extension. */
export function noteName(path: string): string {
  const segment = lastSegment(path);
  return segment.toLowerCase().endsWith(".md") ? segment.slice(0, -3) : segment;
}

/**
 * What the version is called when the person recording it did not say.
 *
 * Names what was in it, which is the only thing known without being told. The list this appears
 * in already shows when each version was made, so the name does not have to carry that.
 */
export function defaultVersionName(paths: string[]): string {
  if (paths.length === 0) return "";
  const first = noteName(paths[0]);
  if (paths.length === 1) return first;
  const others = paths.length - 1;
  return `${first} and ${others} more`;
}

/** The name to record: what was typed, or the default when nothing was. */
export function versionName(typed: string, paths: string[]): string {
  const trimmed = typed.trim();
  return trimmed === "" ? defaultVersionName(paths) : trimmed;
}

/** How much this version covers, said in one line. */
export function scopeSummary(paths: string[]): string {
  if (paths.length === 0) return "Nothing selected";
  if (paths.length === 1) return "This note only";
  return `${paths.length} notes`;
}

/**
 * What to say once the attempt is over.
 *
 * A record that wrote nothing is not a failure and not a success worth the same words: the note
 * is exactly as it was, and so is its history. Saying "added" would point at a version the list
 * does not contain.
 */
export function outcomeMessage(revision: string | null, paths: string[]): string {
  if (revision !== null) {
    return paths.length === 1
      ? `Added a version of ${noteName(paths[0])}.`
      : `Added a version of ${paths.length} notes.`;
  }
  return paths.length === 1
    ? "Nothing to add — this note hasn't changed since its last version."
    : "Nothing to add — these notes haven't changed since their last version.";
}
