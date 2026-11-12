# Scope

Textree is a library. It knows Markdown, hierarchies and the relations between documents; it does not know any
application that uses it. Documentation, changelog entries, identifiers and tests describe behaviour in Textree's
own terms — never in terms of a particular consuming project.

## In

- A model of a hierarchy of Markdown documents and collections.
- Reading Markdown for links, wiki links, tags and headings.
- A relation graph derived from the documents (backlinks, tags, broken links, orphans).
- Change sets and the checks every change set must pass.
- Threshold measures and a reorganization policy.
- An ingest harness that turns text into a change set with a language model.

## Out

- **Storage.** Reading and committing go through a port the consumer implements.
- **Search and embeddings.** A consumer may search however it likes; Textree places text by walking the hierarchy.
- **Rendering** Markdown to HTML.
- **Ontologies.** Relations are links, backlinks and tags — not typed relations.
- **Accounts, permissions, sync.** These belong to the consumer.

## Guarantees the engine keeps

- A change set is committed whole or not at all, and never over a stale base version.
- No content disappears without the change set saying so.
- Links that resolved before a change still resolve after it.
- A plan that comes from outside is written only if its operations, planned again on the current hierarchy,
  give exactly that plan.
- Textree does not judge whether text is sensitive, and the engine and its MCP server use no network.
- Tags are read from the document body and from front matter that already exists. By default they are written into the body only; writing front matter is a policy the consumer turns on for a store it owns.
