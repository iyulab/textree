/**
 * Turns a raw backend error (Rust `Result<_, String>` text or a thrown JS error) into a
 * user-facing summary while ALWAYS preserving the original text for diagnosis.
 *
 * Principle — data safety, no silent loss: we only rewrite errors we recognize.
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
// Numeric "os error N" tokens keep their trailing ")" so a code matches whole, not as a prefix of
// a longer one — without it "os error 2" would also fire on "os error 21" / "os error 267". Rust
// renders raw OS errors as "<message> (os error N)", so the ")" is always present; the text
// substrings are the cross-platform fallback when the numeric form is absent.
const RULES: Rule[] = [
  // Reaching a repository to back notes up. First, so a generic rule below cannot claim them.
  {
    match: ["refused these credentials"],
    summary:
      "The repository didn't accept this access token. Check that it's correct and allowed to read and write that repository.",
  },
  {
    match: ["no repository at this address"],
    summary: "There's no repository at this address, or the token can't see it. Check the address.",
  },
  {
    match: ["could not reach the remote"],
    summary: "The repository couldn't be reached. Check the address and your internet connection.",
  },
  {
    match: ["unencrypted", "only http and https"],
    summary: "Use a secure address that starts with https://.",
  },
  {
    match: ["not connected to a remote"],
    summary: "This folder isn't set up to back up anywhere.",
  },
  {
    match: ["the remote answered with status", "does not serve a git repository"],
    summary: "The repository site answered with an error. Check the address, or try again later.",
  },
  {
    match: ["os error 13)", "permission denied", "access is denied"],
    summary:
      "You don't have permission to write here. Check that the file or folder isn't read-only or open in another program.",
  },
  {
    match: ["os error 28)", "no space left"],
    summary: "There isn't enough disk space to finish this. Free up some space and try again.",
  },
  {
    // os error 2 (unix) / os error 3 (windows: path not found)
    match: ["os error 2)", "os error 3)", "no such file or directory", "cannot find the path"],
    summary:
      "The file or folder could not be found. It may have been moved or deleted outside the app.",
  },
  {
    match: ["already exists"],
    summary: "An item with that name already exists here. Choose a different name.",
  },
  {
    match: ["invalid name"],
    summary:
      "That name can't be used. Avoid / and \\ and a leading dot, and Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9).",
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
    // Another tool left an operation half-finished in this folder. Naming the operation would
    // mean naming machinery the person never opted into, and they cannot act on the name
    // either — what they can act on is the tool they were using.
    match: ["in the middle of another operation"],
    summary:
      "Another tool left something unfinished in this folder. Finish it there, then try again. Your note is saved on disk — nothing is lost.",
  },
  {
    // A rule in the folder says to skip this file. Refusing is the honest answer: recording it
    // would leave the history without the thing it was asked to keep.
    match: ["covered by an ignore rule"],
    summary:
      "A rule in this folder says to skip this file, so it can't get a version. The note itself is untouched.",
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
