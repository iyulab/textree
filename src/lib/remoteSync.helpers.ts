/**
 * Pure text for backing notes up to a repository and exchanging them with it.
 *
 * Everything here speaks about notes, never about the machinery underneath: what arrived, what
 * was removed, what was left as it is and why. Nothing is said about an exchange that changed
 * nothing — a quiet background exchange stays quiet.
 */

import type { FriendlyError } from "./friendlyError.helpers";
import type { RemoteExchange } from "./ipc";

/** How many note names a sentence lists before it says "and N more". */
export const NAMES_SHOWN = 5;

/** A short label for a repository address: host and path, without scheme, sign-in or `.git`. */
export function repositoryLabel(url: string): string {
  let rest = url.trim();
  const scheme = rest.indexOf("://");
  if (scheme >= 0) rest = rest.slice(scheme + 3);
  const slash = rest.indexOf("/");
  const authority = slash >= 0 ? rest.slice(0, slash) : rest;
  let path = slash >= 0 ? rest.slice(slash) : "";
  const at = authority.lastIndexOf("@");
  const host = at >= 0 ? authority.slice(at + 1) : authority;
  path = path.split(/[?#]/)[0].replace(/\/+$/, "").replace(/\.git$/i, "").replace(/\/+$/, "");
  return `${host}${path}`;
}

/** How a note is named on screen: its file name, without folders or `.md`. */
export function noteDisplayName(path: string): string {
  const base = path.split(/[/\\]/).filter(Boolean).pop() ?? path;
  return base.replace(/\.md$/i, "");
}

/** Names joined for a sentence, at most `NAMES_SHOWN` of them, then "and N more". */
export function nameList(paths: string[]): string {
  const names = paths.map(noteDisplayName);
  if (names.length <= NAMES_SHOWN) return names.join(", ");
  const shown = names.slice(0, NAMES_SHOWN).join(", ");
  return `${shown} and ${names.length - NAMES_SHOWN} more`;
}

function notes(n: number): string {
  return n === 1 ? "1 note" : `${n} notes`;
}

/** One sentence per thing the exchange did that the person should know about. */
export function exchangeMessages(exchange: RemoteExchange): string[] {
  const out: string[] = [];
  const { received, removed, keptBack } = exchange;
  // A note with an alternative is said as such; the rest of what was held (a note one side
  // deleted) has no alternative to look at and stays as it is.
  const alternatives = exchange.alternatives ?? [];
  const held = exchange.held.filter((p) => !alternatives.includes(p));
  if (received.length > 0) {
    out.push(`${notes(received.length)} arrived from your backup.`);
  }
  if (removed.length > 0) {
    out.push(
      `${notes(removed.length)} ${removed.length === 1 ? "was" : "were"} removed there and ` +
        `${removed.length === 1 ? "is" : "are"} in Deleted notes here.`,
    );
  }
  if (held.length > 0) {
    out.push(
      `${notes(held.length)} changed both here and elsewhere — kept as ${held.length === 1 ? "it is" : "they are"} ` +
        `for now: ${nameList(held)}`,
    );
  }
  if (alternatives.length > 0) {
    out.push(
      `${notes(alternatives.length)} also changed elsewhere — the other version is kept as an ` +
        `alternative: ${nameList(alternatives)}`,
    );
  }
  if (keptBack.length > 0) {
    out.push(
      `${notes(keptBack.length)} ${keptBack.length === 1 ? "has" : "have"} changes you haven't added as a ` +
        `version, so what arrived for ${keptBack.length === 1 ? "it" : "them"} is waiting: ${nameList(keptBack)}`,
    );
  }
  return out;
}

/**
 * Whether the exchange left something for the person to look at: notes it could not take in.
 * Those are said with a warning; notes that simply arrived or went are said plainly.
 */
export function needsAttention(exchange: RemoteExchange): boolean {
  return exchange.held.length > 0 || exchange.keptBack.length > 0;
}

/** What the panel says after an exchange the person asked for. Never empty. */
export function manualOutcome(exchange: RemoteExchange): string {
  const said = exchangeMessages(exchange);
  if (said.length > 0) return said.join(" ");
  return "Everything is backed up.";
}

/** "just now", "5 minutes ago", "2 hours ago", "3 days ago". */
export function relativeTime(then: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return minutes === 1 ? "1 minute ago" : `${minutes} minutes ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? "1 hour ago" : `${hours} hours ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/**
 * Whether a failed background exchange is news, given the failure last said for the folder.
 * Opening a folder while offline fails the same way every time; the backup status already shows
 * the risk, so the same failure is said once — until an exchange goes through or it fails for
 * another reason.
 */
export function isNewFailure(said: FriendlyError | null, failure: FriendlyError): boolean {
  return said?.summary !== failure.summary;
}
