# Changelog

All notable changes to Textree are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).
Entries describe changes in Textree's own terms — never in terms of a particular consuming project
(see [docs/SCOPE.md](docs/SCOPE.md)).

## [Unreleased]

### Changed

- A tag has to start with a letter or an underscore. Written forms that start with a digit
  (`#123`, `#1577th`, `#2024-goals`) are no longer read as tags; front matter tags are not
  affected.
- A wiki link that starts with a slash (`[[/name]]`) names a path from the root, so a document in
  the root can be told apart from a nearer document with the same name.
- A collapsed reference link (`[text][]`) now covers its trailing brackets in its text and range.
- A saved plan has format version 2: it carries the operations it was made from (`ops`, e.g.
  `{"op": "move", "from": …, "to": …}`) and the policy it relies on (`policy`: `break_links`,
  recorded only when the plan does leave a link broken, `max_touched`, `max_bytes`). `textree apply`
  first refuses a stale plan, then plans the operations again on the folder as it is and writes
  only when that gives exactly the saved plan — an edited plan is refused with
  `refused [plan]: …` and nothing is written. Version 1 plans, which carry no operations, are
  refused; make them again.
- A change set may list `new_broken` (links in new text that do not resolve) and `orphaned`
  (documents left with no links to them); both are reported, not refused, and absent when empty.
- Names, paths, heading fragments and tags compare in Unicode normalization form C. A link written in
  one form now finds a document whose file name is stored in the other (for example a decomposed
  Korean or accented name on disk and a precomposed one typed in a link); tags in both forms are one
  tag, shown in form C. A rewritten link keeps the form its writer used, and paths in the graph keep
  the stored spelling. Names that differ only in normalization form clash.
- A change set has a `creates` list (new documents, never replacing a file) and a write can carry a
  `note` that the plan shows; both are optional in a saved plan.

### Added

- Two operations that put text in: `Op::Add { path, content }` creates a document (never over an
  existing one; a name that would take over another document's name links rewrites those links to
  a path), and `Op::Replace { path, content }` replaces the whole text of one (its old text is the
  declared removal; links from other documents keep working, and a replacement that drops a
  heading another document links to is refused). A text that does not end with a line break gets
  one. On the command line: `textree add <folder> <path> <file>` and
  `textree replace <folder> <path> <file>` (`-` reads standard input).
- Scope and planned contracts (design stage).
- `textree` crate: reads Markdown documents (links, wiki links, tags, headings, with byte ranges; code is
  never read) and derives a relation graph — resolved links, backlinks, tags, broken links and orphans.
  A wiki link with a bare name that several documents carry goes to the nearest one: the same folder, then
  the deeper shared ancestor folder, then the shorter path, then alphabetical order. Output is
  deterministic.
- `textree-folder` crate: a read port over a folder on disk.
- A command line tool (not published) to look at the result: `textree graph <folder> [--json]` and
  `textree check <folder>`, which lists broken links as `path:line` and exits with 1 when there are any.
- A small corpus with a golden relation graph under `tests/data/corpus`.
- Change sets: moving, renaming and deleting a document, an asset or a folder is planned as a
  checked change set (moves, rewritten documents, deletions, folders left empty). Every link that
  resolved still resolves to the same document; only links whose meaning would change are
  rewritten, and nothing else in a document changes by a byte. A folder that is renamed takes its
  folder document (`name/name.md`) along; relative links that leave the root keep pointing at the
  same place; links that were already broken are left as they were.
- A change is refused when its destination has a hidden (dot-prefixed) piece, since hidden names
  are never read, or goes through a symbolic link, which could lead out of the folder. A change
  that the folder does not match once it is written is undone.
- A write port (`WritePort`) and a folder implementation: a change set is committed whole or not
  at all, refused when what it was planned against has changed, and undone when a step fails.
- Read results keep the byte span of every link destination, wiki link name and reference
  definition; `graph --json` lists the definitions of each document (`definitions`).
- Command line: `textree move`, `textree delete` (plans by default, `--apply` writes,
  `--plan <file>` saves) and `textree apply`.
- Splitting and merging: `textree split <folder> <doc> [--level N]` turns a document into a folder
  document plus one new document per section (`--level` defaults to 2), and
  `textree merge <folder> <into> <from>...` appends documents to another one as sections, with their
  headings one level deeper. Both are planned like any change set (`--apply`, `--plan`, the budget
  options) and committed whole or not at all. Links keep their meaning: a link to a section follows
  the section, a link to a document that was merged goes to the section it became, a name that
  would now mean another document is written as a path, and links that were broken stay as they
  were. Same-document anchors that now cross documents become links. Nothing but links and the
  declared structure (the list of sections, a section title, a heading's level) changes; refused
  when text would be lost or changed (front matter of a merged document, a footnote that crosses
  sections, reference definitions that disagree, a heading that would pass level 6, a name that is
  already taken).
- Heading anchors follow the rule of the common slug algorithm (github-slugger 2.x: lower case, keep
  letters, marks, numbers, `_`, `-` and spaces, spaces become `-`, repeated anchors get `-1`, `-2`).
  A link fragment names a heading by its anchor or by its text; a heading that reads the same as
  another is named by its anchor.
- The reader exposes headings with their level, footnotes and links to a heading of the same
  document.
