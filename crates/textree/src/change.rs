//! Change sets: the only unit in which a hierarchy is changed.
//!
//! A [`ChangeSet`] is what planning an operation produces and what a store commits. It is plain
//! data (JSON-serializable) so that it can be reviewed, saved and applied later.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::port::{content_version, Node, NodeKind};

/// Something that can be done to a hierarchy.
///
/// In JSON an operation is an object with an `op` field: `{"op": "move", "from": …, "to": …}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Move or rename a document, an asset or a folder. `to` is the new full path.
    Move { from: String, to: String },
    /// Delete a document, an asset or a folder with everything in it.
    Delete { path: String },
    /// Split a document at its headings of one level: the document becomes a folder document
    /// and each section a new document in the folder.
    Split { path: String, level: u8 },
    /// Merge documents into another one: each becomes a section of it and is deleted.
    Merge { into: String, from: Vec<String> },
    /// Add a new document with this text. A text that does not end with a line break gets one.
    Add { path: String, content: String },
    /// Replace the whole text of a document. Its old text is the declared removal; the links
    /// that came to it have to keep working, headings they name included.
    Replace { path: String, content: String },
}

/// Limits and switches for planning.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Allow a delete that leaves links pointing at nothing.
    #[serde(default)]
    pub break_links: bool,
    /// The most nodes a change may touch (the named node plus the documents it rewrites).
    #[serde(default)]
    pub max_touched: Option<usize>,
    /// The most bytes of link text a change may rewrite.
    #[serde(default, rename = "max_bytes")]
    pub max_changed_bytes: Option<usize>,
}

/// What a plan records of the policy it was made under: the permissions it needs. Limits
/// (`max_touched`, `max_bytes`) only decide whether a plan is made, so a plan does not carry
/// them — what it carries can all be checked by planning again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// The plan leaves a link broken, which the policy allowed.
    pub break_links: bool,
}

/// A version a change set depends on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadVersion {
    pub path: String,
    pub version: String,
}

/// Rename a node. Entries apply in order; each `from` is a path as it is after the entries
/// before it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub from: String,
    pub to: String,
}

/// Replace the whole text of a document (at its path after the moves).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Write {
    pub path: String,
    pub content: String,
    /// A line for the plan that says what this write is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Remove a document or an asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delete {
    pub path: String,
    /// `document` or `asset`.
    pub kind: String,
    /// Lines of a document that is removed, so a review can see what goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
}

/// One link (or reference definition) that is written differently.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewrite {
    /// The document, at its path after the moves.
    pub path: String,
    pub line: usize,
    pub before: String,
    pub after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A planned change to a hierarchy, checked and ready to commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// The format: 2. (Version 1 plans carried no operations.)
    pub version: u32,
    /// The operations this plan was made from. A plan that came from outside is applied only
    /// when these operations, planned again on the current hierarchy, give the same plan.
    #[serde(default)]
    pub ops: Vec<Op>,
    /// What the plan needs of the policy it was planned under: `break_links` only when the plan
    /// does leave a link broken.
    #[serde(default)]
    pub policy: Permissions,
    /// Identifies the set of nodes the plan was made against; a store refuses the plan when
    /// documents were added or removed since (a new document can change what a name means).
    pub nodes_fingerprint: String,
    /// Every document read (and every asset moved or deleted) with the version it had.
    pub reads: Vec<ReadVersion>,
    pub moves: Vec<Move>,
    /// New documents: paths that do not exist yet.
    #[serde(default)]
    pub creates: Vec<Write>,
    pub writes: Vec<Write>,
    pub deletes: Vec<Delete>,
    /// Folders that the change leaves empty; a store removes them when they really are.
    pub remove_folders: Vec<String>,
    pub rewrites: Vec<Rewrite>,
    /// Links in the new text that do not resolve — allowed, and said so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_broken: Vec<BrokenRef>,
    /// Documents that no longer have any link to them after the change — allowed, and said so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphaned: Vec<String>,
}

/// A link that does not resolve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokenRef {
    pub path: String,
    pub line: usize,
    pub link: String,
}

impl ChangeSet {
    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
            && self.creates.is_empty()
            && self.writes.is_empty()
            && self.deletes.is_empty()
            && self.remove_folders.is_empty()
    }
}

/// A fingerprint of which nodes exist (not of what they contain).
pub fn nodes_fingerprint(nodes: &[Node]) -> String {
    let mut lines: Vec<String> = nodes
        .iter()
        .map(|n| {
            let kind = match n.kind {
                NodeKind::Collection => 'c',
                NodeKind::Document => 'd',
                NodeKind::Asset => 'a',
            };
            format!("{kind}{}", n.path)
        })
        .collect();
    lines.sort();
    content_version(lines.join("\n").as_bytes())
}

/// The invariants every change set has to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// `I1` content kept · `I2` links intact · `I3` nothing left empty · `I4` budget · `I5` names.
    pub invariant: &'static str,
    pub path: String,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(
                f,
                "refused [{}] {}:{}: {}",
                self.invariant, self.path, line, self.message
            ),
            None => write!(
                f,
                "refused [{}] {}: {}",
                self.invariant, self.path, self.message
            ),
        }
    }
}

/// Why a change could not be planned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The request itself is wrong (a path that does not exist, leaves the root, …).
    Usage(String),
    /// The change would break an invariant; nothing is planned.
    Refused(Vec<Violation>),
}

/// Why a store did not commit a change set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// The change set itself is malformed or unsafe (a path that leaves the root, …).
    Invalid(String),
    /// What the plan was made against has changed; nothing was written.
    Stale(Vec<String>),
    /// A path the change needs is taken; nothing was written.
    Conflict(Vec<String>),
    /// Writing failed part-way; everything was put back.
    Failed { path: String, message: String },
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitError::Invalid(why) => write!(f, "invalid change set: {why}"),
            CommitError::Stale(paths) => {
                write!(f, "stale: {}", paths.join(", "))
            }
            CommitError::Conflict(paths) => write!(f, "conflict: {}", paths.join(", ")),
            CommitError::Failed { path, message } => write!(f, "failed at {path}: {message}"),
        }
    }
}

/// The paths a change set depends on that changed since it was made: documents read at another
/// version, or any document when the set of nodes is not what it was.
pub fn stale_paths(nodes: &[Node], changes: &ChangeSet) -> Vec<String> {
    let mut stale: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let current: std::collections::HashMap<&str, &Node> =
        nodes.iter().map(|n| (n.path.as_str(), n)).collect();
    for read in &changes.reads {
        match current.get(read.path.as_str()) {
            Some(n) if n.version == read.version => {}
            _ => {
                stale.insert(read.path.clone());
            }
        }
    }
    if nodes_fingerprint(nodes) != changes.nodes_fingerprint {
        let read: std::collections::BTreeSet<&str> =
            changes.reads.iter().map(|r| r.path.as_str()).collect();
        for n in nodes {
            if n.kind == NodeKind::Document && !read.contains(n.path.as_str()) {
                stale.insert(n.path.clone());
            }
        }
        if stale.is_empty() {
            stale.insert("(the set of files has changed)".to_string());
        }
    }
    stale.into_iter().collect()
}

/// A store that can take a change set. All of it or none of it.
pub trait WritePort {
    /// Applies the change set. Returns the number of nodes it wrote, moved or removed.
    fn commit(&self, changes: &ChangeSet) -> Result<usize, CommitError>;
}

/// Checks that every path in a change set stays inside the root: relative, `/`-separated, no
/// empty, `.` or `..` pieces. Meant for change sets that came from outside (a saved file).
pub fn validate_paths(changes: &ChangeSet) -> Result<(), String> {
    let mut all: Vec<&str> = Vec::new();
    all.extend(changes.reads.iter().map(|r| r.path.as_str()));
    for m in &changes.moves {
        all.push(&m.from);
        all.push(&m.to);
    }
    all.extend(changes.writes.iter().map(|w| w.path.as_str()));
    all.extend(changes.creates.iter().map(|w| w.path.as_str()));
    all.extend(changes.deletes.iter().map(|d| d.path.as_str()));
    all.extend(changes.remove_folders.iter().map(String::as_str));
    all.extend(changes.rewrites.iter().map(|r| r.path.as_str()));
    for path in all {
        if let Err(why) = crate::path::check_inside_root(path) {
            return Err(format!("{path:?}: {why}"));
        }
        // Hidden names are never read, so a plan made by Textree never names one.
        if path.split('/').any(|piece| piece.starts_with('.')) {
            return Err(format!("{path:?}: has a hidden piece (starts with a dot)"));
        }
    }
    Ok(())
}
