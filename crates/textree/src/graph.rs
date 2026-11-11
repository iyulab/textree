//! The relation graph: what a hierarchy of documents says about itself.
//!
//! The graph is derived data. It is rebuilt from the documents every time and nothing in it is
//! stored; the same input always gives the same graph.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::path::{dir_of, name_of, stem_of};
use crate::port::{NodeKind, ReadPort};
use crate::read::{read_document, Heading, Range, RawLink, Syntax, TagUse};
use crate::resolve::{Index, Resolution};

/// How a resolved link was written, and what it points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    /// `[[name]]`
    Wiki,
    /// `![[name]]`
    Embed,
    /// `[text](path)`
    Path,
    /// A path link whose target is an asset (an image, an attachment).
    PathAsset,
}

/// A reference definition (`[label]: destination`) of a document.
#[derive(Clone, Debug, Serialize)]
pub struct Definition {
    pub label: String,
    pub target: String,
    pub range: Range,
    pub line: usize,
}

/// What was read from one document.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentInfo {
    pub path: String,
    pub version: String,
    pub headings: Vec<Heading>,
    pub tags: Vec<TagUse>,
    /// Where the destinations of reference links are written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<Definition>,
}

/// A link from a document, with what it points at (`to` is `null` when it is broken).
#[derive(Clone, Debug, Serialize)]
pub struct Link {
    pub from: String,
    pub link: String,
    pub to: Option<String>,
    pub kind: LinkKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub range: Range,
    pub line: usize,
    /// Other documents the same name could have meant, nearest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
}

/// A link that does not resolve.
#[derive(Clone, Debug, Serialize)]
pub struct BrokenLink {
    pub from: String,
    pub link: String,
    pub kind: LinkKind,
    pub range: Range,
    pub line: usize,
}

/// A node the engine could not read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReadError {
    pub path: String,
    pub message: String,
}

/// The relation graph of a hierarchy.
#[derive(Clone, Debug, Serialize)]
pub struct Graph {
    pub documents: Vec<DocumentInfo>,
    pub assets: Vec<String>,
    pub links: Vec<Link>,
    pub tags: BTreeMap<String, Vec<String>>,
    pub backlinks: BTreeMap<String, Vec<String>>,
    pub orphans: Vec<String>,
    pub broken: Vec<BrokenLink>,
    pub errors: Vec<ReadError>,
}

/// Reads every document of a hierarchy and builds its graph. A document that cannot be read is
/// reported in `errors` and the rest are still processed.
pub fn build_graph(port: &dyn ReadPort) -> Graph {
    let listing = port.list();
    let index = Index::new(&listing.nodes);

    let mut errors: Vec<ReadError> = listing
        .issues
        .iter()
        .map(|i| ReadError {
            path: i.path.clone(),
            message: i.message.clone(),
        })
        .collect();

    let mut doc_nodes: Vec<_> = listing
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Document)
        .collect();
    doc_nodes.sort_by(|a, b| a.path.cmp(&b.path));
    let mut assets: Vec<String> = listing
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Asset)
        .map(|n| n.path.clone())
        .collect();
    assets.sort();

    let mut documents = Vec::new();
    let mut links = Vec::new();
    let mut broken = Vec::new();

    for node in doc_nodes {
        let bytes = match port.read(&node.path) {
            Ok(bytes) => bytes,
            Err(message) => {
                errors.push(ReadError {
                    path: node.path.clone(),
                    message: format!("cannot read: {message}"),
                });
                continue;
            }
        };
        let (offset, body) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(rest) => (3, rest),
            None => (0, &bytes[..]),
        };
        let text = match std::str::from_utf8(body) {
            Ok(text) => text,
            Err(_) => {
                errors.push(ReadError {
                    path: node.path.clone(),
                    message: "cannot read: not valid UTF-8".into(),
                });
                continue;
            }
        };
        let reading = read_document(text, offset);
        for raw in &reading.links {
            match index.resolve(&node.path, raw) {
                Resolution::Outside => {}
                Resolution::Broken => {
                    let kind = syntax_kind(raw, false);
                    broken.push(BrokenLink {
                        from: node.path.clone(),
                        link: raw.text.clone(),
                        kind,
                        range: raw.range,
                        line: raw.line,
                    });
                    links.push(make_link(&node.path, raw, None, kind, Vec::new()));
                }
                Resolution::Target {
                    path,
                    kind,
                    alternatives,
                } => {
                    let link_kind = syntax_kind(raw, kind == NodeKind::Asset);
                    links.push(make_link(
                        &node.path,
                        raw,
                        Some(path),
                        link_kind,
                        alternatives,
                    ));
                }
            }
        }
        documents.push(DocumentInfo {
            path: node.path.clone(),
            version: node.version.clone(),
            headings: reading.headings,
            tags: reading.tags,
            definitions: reading
                .defs
                .iter()
                .map(|d| Definition {
                    label: d.label.clone(),
                    target: d.target.clone(),
                    range: d.range,
                    line: d.line,
                })
                .collect(),
        });
    }

    // Backlinks: target document -> distinct source documents, never the document itself.
    let mut backlinks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let document_paths: BTreeSet<&str> = documents.iter().map(|d| d.path.as_str()).collect();
    for link in &links {
        if let Some(to) = &link.to {
            if to != &link.from && document_paths.contains(to.as_str()) {
                backlinks
                    .entry(to.clone())
                    .or_default()
                    .insert(link.from.clone());
            }
        }
    }
    let orphans: Vec<String> = documents
        .iter()
        .filter(|d| !backlinks.contains_key(&d.path) && !is_collection_document(&d.path))
        .map(|d| d.path.clone())
        .collect();

    Graph {
        tags: collect_tags(&documents),
        documents,
        assets,
        links,
        backlinks: backlinks
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
        orphans,
        broken,
        errors,
    }
}

fn syntax_kind(raw: &RawLink, target_is_asset: bool) -> LinkKind {
    match raw.syntax {
        Syntax::Wiki => LinkKind::Wiki,
        Syntax::Embed => LinkKind::Embed,
        Syntax::Path if target_is_asset => LinkKind::PathAsset,
        Syntax::Path => LinkKind::Path,
    }
}

fn make_link(
    from: &str,
    raw: &RawLink,
    to: Option<String>,
    kind: LinkKind,
    alternatives: Vec<String>,
) -> Link {
    Link {
        from: from.to_string(),
        link: raw.text.clone(),
        to,
        kind,
        heading: raw.heading.clone(),
        alias: raw.alias.clone(),
        range: raw.range,
        line: raw.line,
        alternatives,
    }
}

/// A collection document is the document that stands for its own folder: `notes/notes.md`.
fn is_collection_document(path: &str) -> bool {
    let dir = dir_of(path);
    !dir.is_empty() && name_of(dir) == stem_of(name_of(path))
}

/// Tag -> documents. Tags compare without regard to case; the spelling shown is the one used
/// most often (ties: the alphabetically first).
fn collect_tags(documents: &[DocumentInfo]) -> BTreeMap<String, Vec<String>> {
    let mut spellings: HashMap<String, HashMap<&str, usize>> = HashMap::new();
    let mut users: HashMap<String, BTreeSet<&str>> = HashMap::new();
    for doc in documents {
        for tag in &doc.tags {
            let key = tag.tag.to_lowercase();
            *spellings
                .entry(key.clone())
                .or_default()
                .entry(tag.tag.as_str())
                .or_insert(0) += 1;
            users.entry(key).or_default().insert(doc.path.as_str());
        }
    }
    let mut out = BTreeMap::new();
    for (key, counts) in spellings {
        let shown = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(s, _)| (*s).to_string())
            .unwrap_or(key.clone());
        let docs = users
            .remove(&key)
            .unwrap_or_default()
            .into_iter()
            .map(String::from)
            .collect();
        out.insert(shown, docs);
    }
    out
}
