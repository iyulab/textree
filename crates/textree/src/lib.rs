//! Textree: a deterministic engine for hierarchical Markdown.
//!
//! It reads documents (links, wiki links, tags, headings — never code), resolves what links
//! point at, and derives a relation graph: backlinks, tags, broken links and orphans.
//! It knows nothing about storage; a [`ReadPort`] tells it what is in the hierarchy.
//!
//! ```
//! use textree::{build_graph, MemoryStore};
//!
//! let store = MemoryStore::new()
//!     .with_file("a.md", "See [[b]].")
//!     .with_file("b.md", "Hello #greeting");
//! let graph = build_graph(&store);
//! assert_eq!(graph.links[0].to.as_deref(), Some("b.md"));
//! assert_eq!(graph.backlinks["b.md"], vec!["a.md".to_string()]);
//! assert_eq!(graph.orphans, vec!["a.md".to_string()]);
//! ```

mod anchors;
mod change;
mod graph;
pub mod path;
mod plan;
mod port;
mod read;
mod resolve;

pub use anchors::{anchors_of, file_stem_of, github_slug};
pub use change::{
    nodes_fingerprint, stale_paths, validate_paths, ChangeSet, CommitError, Delete, Move, Op,
    PlanError, Policy, ReadVersion, Rewrite, Violation, Write, WritePort,
};
pub use graph::{
    build_graph, BrokenLink, Definition, DocumentInfo, Graph, Link, LinkKind, ReadError,
};
pub use plan::{plan, recheck, Plan, RecheckError, Snapshot};
pub use port::{
    content_version, is_document_path, Issue, Listing, MemoryStore, Node, NodeKind, ReadPort,
};
pub use read::{read_document, Footnote, Heading, Range, RawLink, Reading, RefDef, Syntax, TagUse};
pub use resolve::{Index, Resolution};
