# Changelog

All notable changes to Textree are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).
Entries describe changes in Textree's own terms — never in terms of a particular consuming project
(see [docs/SCOPE.md](docs/SCOPE.md)).

## [Unreleased]

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
