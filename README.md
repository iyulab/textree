# Textree

**A knowledge engine for hierarchical Markdown.** Textree keeps a body of Markdown documents organized as
knowledge: a hierarchy first, and on top of it the relations between documents — links, backlinks and tags.
Hand it new text and it decides where that text belongs, which documents it changes, and when the hierarchy
itself should be reorganized; then it checks the result before anything is stored.

Textree does not do storage. A consumer tells it how to read the hierarchy and how to commit a change set; the
storage behind that may be a folder on disk, a network share or a database.

> **Status: early.** Reading Markdown and the relation graph work and can be tried from a checkout (below);
> nothing is released yet. The rest of the contracts below are the plan, not a published API.

## Try it

```sh
cargo run --release -p textree-cli -- graph path/to/notes          # documents, links, tags, orphans
cargo run --release -p textree-cli -- graph path/to/notes --json   # the full graph, with byte ranges
cargo run --release -p textree-cli -- check path/to/notes          # broken links as path:line; exit 1 if any
```

## What it holds

| Part | What it does |
|---|---|
| **Model** | Two kinds of node: a *collection* (has children) and a *document* (Markdown body). Paths, depth, and a version per node supplied by the store |
| **Markdown reading** | Links, wiki links (`[[name]]`), tags, headings — code is never read as a link |
| **Relation graph** | Backlinks, tag → documents, broken links, orphans. Derived data: it can always be rebuilt from the documents |
| **Change sets** | Add · edit a section · move or rename · split · merge · delete, with links rewritten when a document moves. Every change set is checked: content is preserved or its removal is explicit, links resolve, nothing is orphaned, the change stays within its budget |
| **Thresholds** | Width, depth, length, cohesion and duplication measures that tell when a part of the hierarchy should be reorganized. Values come from a policy the consumer supplies |
| **Ingest harness** | Places new text by walking the hierarchy from the top, reorganizes a subtree when a threshold is crossed, and repairs a change set that fails its checks. Uses a language model through a port |

## Ports

- **Hierarchy store** — read the nodes and their versions; `commit(change set, base version)` applies all of it or
  none of it, and refuses when the base version is stale (the engine then reads again and replans).
- **Language model** — for the ingest harness only. The engine itself is deterministic.

## Modes

- **Propose** (default) — the change set is returned to the consumer, which decides.
- **Quiet** — a change set that passes its checks is committed through the store.

## Out of scope

Search and embeddings, rendering, storage, and ontologies (typed relations) — Textree's relations are links,
backlinks and tags.

## Planned packages

| Package | Contents |
|---|---|
| Rust crate `textree` | The engine |
| NuGet `Textree` | .NET binding of the engine |
| npm `@iyulab/textree` | WebAssembly build of the engine |
| NuGet `Textree.Ingest` | The ingest harness, with an MCP server |

## License

MIT — see [LICENSE](LICENSE).
