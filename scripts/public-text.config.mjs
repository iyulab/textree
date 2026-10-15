/*
 * What this repository keeps out of its public text, on top of the checker's own rules (local paths,
 * private hosts). Read by `tauri-kit-dev public-text --config scripts/public-text.config.mjs`.
 *
 * This repository is public. Text here is read by people who have never seen the working notes it
 * was written from: a reference to an internal decision number, a path from the machine it was
 * written on, or a sentence in a language the rest of the file is not, all read as noise at best.
 * At worst they leak the shape of somewhere else.
 *
 * The rules match shapes, never names: listing the names to keep out of a public repository, in a
 * file committed to it, would publish what the list is meant to withhold.
 *
 * A line that is meant to carry what a rule finds says so with `public-text: allow — <why>` on that
 * line or the one above it.
 */

export default {
  forbidden: [
    {
      // Hangul, Kana and CJK ideographs. The rest of the repository is English; a run of these is
      // either a fixture that means to be non-Latin, or text that was never translated.
      why: "non-Latin text in an English repository",
      re: /[\u1100-\u11ff\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uac00-\ud7af]/u,
    },
    {
      // Decision numbers in both the current (D-74) and the retired (D18) spelling, and the documents
      // they live in. None of these resolve to anything a reader here can open.
      why: "names a document or a decision that only exists outside this repository",
      re: /\b(?:D-\d{1,3}|D\d{1,2}|ADR-\d{4}|cycle-\d+|claudedocs|dev-docs|HANDOFF|constitution)\b/,
    },
    {
      // A home directory, because the segment after it is an account name. Bare drive letters are
      // not matched: `D:/vault/diary` in a path-handling test and a path on someone's machine have
      // the same shape, and what tells them apart is whose it is.
      why: "names someone's home directory, and so the account it belongs to",
      re: /(?:[A-Za-z]:)?[\/](?:Users|home)[\/][A-Za-z0-9_-]/,
    },
    {
      // Mail addresses, minus what looks like one and is not: a file with a scale suffix
      // (`icon@2x.png`), the domains reserved for documentation, and no-reply senders.
      why: "a mail address identifies a person",
      re: /(?<![A-Za-z0-9._%+-])(?!no-?reply@)[A-Za-z0-9._%+-]+@(?!(?:[A-Za-z0-9-]+\.)*(?:example\.(?:com|net|org)|example|invalid|test|localhost)(?![A-Za-z0-9.-]))(?:[A-Za-z0-9-]+\.)+(?!(?:png|jpe?g|gif|svg|webp|ico|css|js|ts|md|json|html)(?![A-Za-z0-9.-]))[A-Za-z]{2,}\b/,
    },
  ],
  allowed: [
    // Generated from the dependency graph: other people's names and licences.
    "THIRD-PARTY-NOTICES.md:",
    "THIRD-PARTY-LICENSES.txt:",
    // License texts taken from other projects' sources, verbatim: their authors' names and addresses.
    "scripts/notice-texts/",
    // The subject of these tests is non-Latin text: a language written without spaces is split into
    // searchable tokens. Latin fixtures would pass while testing nothing.
    "src-tauri/src/search.rs:",
    // Ignore entries that keep the notes of a surrounding workspace out of this repository.
    ".gitignore:dev-docs/",
    ".gitignore:claudedocs/",
    // This file has to contain the shapes it looks for.
    "scripts/public-text.config.mjs:",
  ],
};
