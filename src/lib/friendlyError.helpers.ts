/**
 * Turns a raw backend error (Rust `Result<_, String>` text or a thrown JS error) into a
 * user-facing summary while ALWAYS preserving the original text for diagnosis.
 *
 * Principle (constitution: data safety, no silent loss): we only rewrite errors we recognize.
 * Anything unknown passes through verbatim — we never swallow or replace a diagnostic we can't map.
 */
export interface FriendlyError {
  /** A plain-language sentence to show the user. Equals `raw` when the error is unrecognized. */
  summary: string;
  /** The original error text, kept verbatim so technical detail is never lost. */
  raw: string;
}

interface Rule {
  /** Lowercased substrings; the rule matches when ANY is present in the raw text. */
  match: string[];
  summary: string;
}

// Order matters only when substrings could overlap; current rules are disjoint enough.
const RULES: Rule[] = [
  {
    match: ["os error 13", "permission denied", "access is denied"],
    summary:
      "You don't have permission to write here. Check that the file or folder isn't read-only or open in another program.",
  },
  {
    match: ["os error 28", "no space left"],
    summary: "There isn't enough disk space to finish this. Free up some space and try again.",
  },
  {
    // os error 2 (unix) / os error 3 (windows: path not found)
    match: ["os error 2", "os error 3", "no such file or directory", "cannot find the path"],
    summary:
      "The file or folder could not be found. It may have been moved or deleted outside the app.",
  },
  {
    match: ["already exists"],
    summary: "An item with that name already exists here. Choose a different name.",
  },
  {
    match: ["canopy failed", "failed to start canopy"],
    summary:
      "The publishing tool couldn't finish. The publish bundle may be missing — see the release notes.",
  },
  {
    match: ["must be outside the vault", "must not contain the vault"],
    summary: "Pick a folder outside your vault to publish the site into.",
  },
  {
    match: ["outside the vault", "path is outside"],
    summary: "That location is outside the vault and can't be used.",
  },
];

/** Extract a string from any caught value (string, Error, or arbitrary object). */
function toRawString(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function friendlyError(e: unknown): FriendlyError {
  const raw = toRawString(e);
  const haystack = raw.toLowerCase();
  for (const rule of RULES) {
    if (rule.match.some((m) => haystack.includes(m))) {
      return { summary: rule.summary, raw };
    }
  }
  // Unrecognized: surface the raw text as the summary so nothing is hidden.
  // Empty input still needs a non-empty message.
  return { summary: raw.trim() === "" ? "Something went wrong." : raw, raw };
}
