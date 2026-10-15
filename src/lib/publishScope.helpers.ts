/**
 * Which notes and folders go out when the folder is published. Nothing goes out unless chosen;
 * choosing a folder covers everything under it; the nearest explicit choice wins.
 */
export interface PublishChoice {
  /** `/`-separated, relative to the notes folder; "" is the whole folder. */
  path: string;
  include: boolean;
}

/** ASCII-only lowercasing, matching the Rust side's `eq_ignore_ascii_case`. */
function asciiLower(s: string): string {
  return s.replace(/[A-Z]+/g, (m) => m.toLowerCase());
}

function covers(choice: string, rel: string): boolean {
  if (choice === "") return true;
  const c = asciiLower(choice);
  const r = asciiLower(rel);
  return r === c || r.startsWith(`${c}/`);
}

/** Whether `rel` goes out under `choices`. */
export function isIncluded(rel: string, choices: PublishChoice[]): boolean {
  let nearest: PublishChoice | null = null;
  for (const choice of choices) {
    if (covers(choice.path, rel) && (nearest === null || choice.path.length >= nearest.path.length)) {
      nearest = choice;
    }
  }
  return nearest?.include ?? false;
}

/** The explicit choice recorded for exactly `rel`, if any. */
export function explicitChoice(rel: string, choices: PublishChoice[]): PublishChoice | null {
  const r = asciiLower(rel);
  return choices.find((c) => asciiLower(c.path) === r) ?? null;
}

/**
 * `abs` relative to the notes folder `root`, `/`-separated; "" for the folder itself, null when
 * `abs` is outside it. Accepts backslashes and matches the prefix ASCII case-insensitively.
 */
export function relativeTo(root: string, abs: string): string | null {
  const r = root.replace(/\\/g, "/").replace(/\/+$/, "");
  const a = abs.replace(/\\/g, "/");
  if (asciiLower(a) === asciiLower(r)) return "";
  return asciiLower(a).startsWith(`${asciiLower(r)}/`) ? a.slice(r.length + 1) : null;
}
