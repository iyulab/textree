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

### Added

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
