# Textree

**A knowledge engine for hierarchical Markdown.** Textree keeps a body of Markdown documents organized as
knowledge: a hierarchy first, and on top of it the relations between documents — links, backlinks and tags.
Hand it new text and it decides where that text belongs, which documents it changes, and when the hierarchy
itself should be reorganized; then it checks the result before anything is stored.

Textree does not do storage. A consumer tells it how to read the hierarchy and how to commit a change set; the
storage behind that may be a folder on disk, a network share or a database.

> **Status: early.** Reading Markdown, the relation graph, and moving, renaming, deleting, splitting,
> merging, adding and replacing documents with links kept intact work and can be tried from a checkout
> (below), on the command line or as an MCP server; nothing is released yet. The ingest harness and the
> thresholds below are the plan, not a published API.

## Try it

```sh
cargo run --release -p textree-cli -- graph path/to/notes          # documents, links, tags, orphans
cargo run --release -p textree-cli -- graph path/to/notes --json   # the full graph, with byte ranges
cargo run --release -p textree-cli -- check path/to/notes          # broken links as path:line; exit 1 if any
cargo run --release -p textree-cli -- move path/to/notes a.md sub/a.md     # show the plan, change nothing
cargo run --release -p textree-cli -- move path/to/notes a.md sub/a.md --apply
cargo run --release -p textree-cli -- delete path/to/notes old.md --apply  # refused if links would break
cargo run --release -p textree-cli -- split path/to/notes trip.md --apply   # one document per ## section
cargo run --release -p textree-cli -- merge path/to/notes a.md b.md --apply # b.md becomes a section of a.md
cargo run --release -p textree-cli -- add path/to/notes new.md text.md      # a new document from a file (- for stdin)
cargo run --release -p textree-cli -- move path/to/notes a.md b.md --plan plan.json  # save a plan…
cargo run --release -p textree-cli -- apply path/to/notes plan.json         # …and apply it if it still holds
```

Unicode names compare in normalization form C, so a link typed as `café` finds a file whose name the file
system stores decomposed (`cafe` followed by a combining accent), and the other way round. A path given
to a command or a tool is read the same way.

## MCP server

An agent that speaks MCP can work on one folder through a stdio server (needs the .NET 10 SDK and Rust):

```sh
dotnet run --project dotnet/src/Textree.Mcp -- --root path/to/notes
```

As a client entry: `{"command": "dotnet", "args": ["<path>/Textree.Mcp.dll", "--root", "<folder>"]}`.

| Tools | |
|---|---|
| `tree`, `read_document`, `graph`, `check` | read the folder |
| `move`, `delete`, `split`, `merge`, `add_document`, `replace_document` | return a plan, write nothing |
| `apply_plan` | the only tool that writes: it takes a plan unchanged, and refuses it if it was edited or the folder changed since |

Every tool declares its MCP safety hints. Results are the engine's JSON. The server uses no network, reads no
configuration from the environment, and writes only inside `--root`.

**Textree does not judge sensitivity.** It treats every document the same way; whether a folder may hold
secrets, and who may read it, is up to whoever runs it.

For Linux (glibc, x64 or arm64) the native library can be built in a container:
`dotnet/scripts/build-linux-native.sh amd64|arm64`, then
`dotnet publish dotnet/src/Textree.Mcp -p:TextreeNativeLibrary=<the .so>`.

## What it holds

| Part | What it does |
|---|---|
| **Model** | Two kinds of node: a *collection* (has children) and a *document* (Markdown body). Paths, depth, and a version per node supplied by the store |
| **Markdown reading** | Links, wiki links (`[[name]]`), tags, headings — code is never read as a link |
| **Relation graph** | Backlinks, tag → documents, broken links, orphans. Derived data: it can always be rebuilt from the documents |
| **Change sets** | Move or rename, delete, split a document into sections, merge documents, add a document and replace its text, with links rewritten so they keep meaning what they meant (edit a section is planned). A saved plan carries its operations and is applied only if they still give exactly that plan. Every change set is checked: content is preserved or its removal is explicit, links resolve, nothing is orphaned, the change stays within its budget (a count of touched nodes and rewritten bytes). A change is applied whole or not at all, and never over a stale plan |
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
| NuGet `Textree` | .NET binding of the engine (over a C interface, `textree-ffi`) |
| NuGet `Textree.Mcp` | The MCP server over a folder |
| npm `@iyulab/textree` | WebAssembly build of the engine |
| NuGet `Textree.Ingest` | The ingest harness, adding its tools to the MCP server |

## License

MIT — see [LICENSE](LICENSE).
