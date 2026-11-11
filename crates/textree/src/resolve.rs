//! Link resolution: what a link points at.
//!
//! * A path link is resolved against the folder of the linking document, and names exactly one
//!   place.
//! * A wiki link with a `/` is matched against whole paths (case-insensitive, `.md` optional).
//! * A wiki link without a `/` is matched by name. When several documents carry that name, the
//!   one nearest to the linking document wins: the same folder first, then the deeper shared
//!   ancestor folder, then the shorter path, then alphabetical order. The result does not depend
//!   on the order in which documents were listed.

use std::collections::HashMap;

use crate::path::{dir_of, name_of, percent_decode, resolve_relative, stem_of};
use crate::port::{Node, NodeKind};
use crate::read::{RawLink, Syntax};

/// The outcome of resolving one link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Points inside the hierarchy.
    Target {
        path: String,
        kind: NodeKind,
        alternatives: Vec<String>,
    },
    /// Points inside the hierarchy at something that is not there.
    Broken,
    /// Leaves the hierarchy (a web address, a path outside the root) or points at a heading of
    /// its own document: not part of the graph.
    Outside,
}

/// A lookup over the nodes of a hierarchy.
pub struct Index {
    kinds: HashMap<String, NodeKind>,
    by_lower_path: HashMap<String, Vec<String>>,
    by_name: HashMap<String, Vec<String>>,
}

impl Index {
    pub fn new(nodes: &[Node]) -> Self {
        let mut kinds = HashMap::new();
        let mut by_lower_path: HashMap<String, Vec<String>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for node in nodes {
            kinds.insert(node.path.clone(), node.kind);
            if node.kind == NodeKind::Collection {
                continue;
            }
            by_lower_path
                .entry(node.path.to_lowercase())
                .or_default()
                .push(node.path.clone());
            let name = name_of(&node.path).to_lowercase();
            by_name
                .entry(name.clone())
                .or_default()
                .push(node.path.clone());
            if node.kind == NodeKind::Document {
                let stem = stem_of(&name).to_string();
                if stem != name {
                    by_name.entry(stem).or_default().push(node.path.clone());
                }
            }
        }
        for list in by_lower_path.values_mut().chain(by_name.values_mut()) {
            list.sort();
            list.dedup();
        }
        Index {
            kinds,
            by_lower_path,
            by_name,
        }
    }

    /// Resolves a link found in the document at `from`.
    pub fn resolve(&self, from: &str, link: &RawLink) -> Resolution {
        match link.syntax {
            Syntax::Path => self.resolve_path(from, &link.target),
            Syntax::Wiki | Syntax::Embed => self.resolve_wiki(from, &link.target),
        }
    }

    fn resolve_path(&self, from: &str, dest: &str) -> Resolution {
        let dest = dest.trim().trim_start_matches('<').trim_end_matches('>');
        let dest = dest.replace('\\', "/");
        if crate::path::is_external(&dest) {
            return Resolution::Outside;
        }
        // Only the part before a query counts; a link that is only a fragment points at itself.
        let dest = dest.split('?').next().unwrap_or("");
        if dest.is_empty() {
            return Resolution::Outside;
        }
        let decoded = percent_decode(dest);
        match resolve_relative(dir_of(from), &decoded) {
            None => Resolution::Outside,
            Some(path) => {
                let path = path.trim_end_matches('/').to_string();
                match self.kinds.get(&path) {
                    Some(&kind) => Resolution::Target {
                        path,
                        kind,
                        alternatives: Vec::new(),
                    },
                    None => Resolution::Broken,
                }
            }
        }
    }

    fn resolve_wiki(&self, from: &str, name: &str) -> Resolution {
        let name = name.trim().replace('\\', "/");
        let lower = name.trim_matches('/').to_lowercase();
        if lower.is_empty() {
            return Resolution::Broken;
        }
        let candidates: Vec<String> = if lower.contains('/') {
            let mut found = self.by_lower_path.get(&lower).cloned().unwrap_or_default();
            if found.is_empty() {
                found = self
                    .by_lower_path
                    .get(&format!("{lower}.md"))
                    .cloned()
                    .unwrap_or_default();
            }
            found
        } else {
            self.by_name.get(&lower).cloned().unwrap_or_default()
        };
        if candidates.is_empty() {
            return Resolution::Broken;
        }
        let mut ranked = candidates;
        let from_dir: Vec<&str> = split_dir(dir_of(from));
        ranked.sort_by_key(|a| rank(&from_dir, a));
        let path = ranked.remove(0);
        let kind = self.kinds.get(&path).copied().unwrap_or(NodeKind::Document);
        Resolution::Target {
            path,
            kind,
            alternatives: ranked,
        }
    }
}

fn split_dir(dir: &str) -> Vec<&str> {
    if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    }
}

/// Smaller is nearer: (not in the same folder, fewer shared folders, longer path, path).
fn rank(from_dir: &[&str], candidate: &str) -> (bool, std::cmp::Reverse<usize>, usize, String) {
    let cand_dir = split_dir(dir_of(candidate));
    let shared = from_dir
        .iter()
        .zip(cand_dir.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let same_folder = from_dir.len() == cand_dir.len() && shared == from_dir.len();
    let components = candidate.split('/').count();
    (
        !same_folder,
        std::cmp::Reverse(shared),
        components,
        candidate.to_string(),
    )
}
