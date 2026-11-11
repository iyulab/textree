//! The read port: how a store tells the engine what is in the hierarchy.
//!
//! The engine never touches a file system. A consumer implements [`ReadPort`] over whatever it
//! stores documents in; [`MemoryStore`] is a small in-memory implementation.

use std::collections::BTreeMap;

use serde::Serialize;

/// What a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// Has children.
    Collection,
    /// A Markdown document.
    Document,
    /// Anything else (an image, an attachment). A link can point at it; it is never read.
    Asset,
}

/// One node of the hierarchy as the store sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// Path relative to the root, `/`-separated, no leading slash.
    pub path: String,
    pub kind: NodeKind,
    /// An opaque value that changes when the node changes. The engine only compares versions.
    pub version: String,
}

/// Something the store could not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// The result of listing a store.
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub nodes: Vec<Node>,
    pub issues: Vec<Issue>,
}

/// Read access to a hierarchy.
pub trait ReadPort {
    /// Every node, in any order.
    fn list(&self) -> Listing;
    /// The bytes of one document.
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
}

/// Whether a path names a Markdown document.
pub fn is_document_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].eq_ignore_ascii_case("md"),
        _ => false,
    }
}

/// A stable version for a byte string (FNV-1a, 64 bit).
pub fn content_version(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

/// A hierarchy held in memory. Collections are inferred from the file paths.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file; the path is relative and `/`-separated.
    pub fn with_file(mut self, path: &str, content: impl AsRef<[u8]>) -> Self {
        self.files
            .insert(path.to_string(), content.as_ref().to_vec());
        self
    }
}

impl ReadPort for MemoryStore {
    fn list(&self) -> Listing {
        let mut nodes = Vec::new();
        let mut collections = std::collections::BTreeSet::new();
        for (path, bytes) in &self.files {
            let mut dir = path.as_str();
            while let Some(slash) = dir.rfind('/') {
                dir = &dir[..slash];
                collections.insert(dir.to_string());
            }
            let kind = if is_document_path(path) {
                NodeKind::Document
            } else {
                NodeKind::Asset
            };
            nodes.push(Node {
                path: path.clone(),
                kind,
                version: content_version(bytes),
            });
        }
        for dir in collections {
            nodes.push(Node {
                path: dir,
                kind: NodeKind::Collection,
                version: String::new(),
            });
        }
        Listing {
            nodes,
            issues: Vec::new(),
        }
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| "no such file".to_string())
    }
}
