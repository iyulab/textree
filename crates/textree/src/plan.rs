//! Planning a change: from an operation and a snapshot of the hierarchy to a checked
//! [`ChangeSet`].
//!
//! Planning is pure. It reads a [`Snapshot`], works out where every node ends up, re-resolves
//! every link of every document against the hierarchy as it would be, and rewrites exactly the
//! links whose meaning would otherwise change — nothing else in a document is touched. The
//! result is then checked by rebuilding the relation graph of the planned state and comparing
//! it with the graph of the current state.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range as StdRange;

use crate::change::{
    nodes_fingerprint, ChangeSet, Delete, Move, Op, PlanError, Policy, ReadVersion, Rewrite,
    Violation, Write,
};
use crate::graph::{build_graph, Graph};
use crate::path::{
    dir_of, encode_destination, is_external, name_of, outside_location, percent_decode,
    relative_between, relative_to_outside, stem_of,
};
use crate::port::{Issue, Listing, Node, NodeKind, ReadPort};
use crate::read::{read_document, RawLink, Reading, Syntax};
use crate::resolve::{Index, Resolution};

mod restructure;

/// What planning needs to know about a hierarchy: its nodes and every readable document.
pub struct Snapshot {
    pub nodes: Vec<Node>,
    files: BTreeMap<String, Vec<u8>>,
    readings: BTreeMap<String, (usize, Reading)>,
    /// Documents that could not be read, with why.
    pub unreadable: Vec<Issue>,
}

impl Snapshot {
    /// Reads every document of the hierarchy.
    pub fn read(port: &dyn ReadPort) -> Snapshot {
        let listing = port.list();
        let mut files = BTreeMap::new();
        let mut readings = BTreeMap::new();
        let mut unreadable = listing.issues.clone();
        for node in listing
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Document)
        {
            let bytes = match port.read(&node.path) {
                Ok(bytes) => bytes,
                Err(message) => {
                    unreadable.push(Issue {
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
            match std::str::from_utf8(body) {
                Ok(text) => {
                    readings.insert(node.path.clone(), (offset, read_document(text, offset)));
                    files.insert(node.path.clone(), bytes);
                }
                Err(_) => unreadable.push(Issue {
                    path: node.path.clone(),
                    message: "cannot read: not valid UTF-8".to_string(),
                }),
            }
        }
        Snapshot {
            nodes: listing.nodes,
            files,
            readings,
            unreadable,
        }
    }

    /// The relation graph of this state.
    pub fn graph(&self) -> Graph {
        build_graph(&SnapshotStore {
            nodes: &self.nodes,
            files: &self.files,
        })
    }
}

/// A read port over nodes and document bytes held in memory.
struct SnapshotStore<'a> {
    nodes: &'a [Node],
    files: &'a BTreeMap<String, Vec<u8>>,
}

impl ReadPort for SnapshotStore<'_> {
    fn list(&self) -> Listing {
        Listing {
            nodes: self.nodes.to_vec(),
            issues: Vec::new(),
        }
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| "not read".to_string())
    }
}

/// A planned change together with the graph the hierarchy will have after it.
#[derive(Clone, Debug)]
pub struct Plan {
    pub changes: ChangeSet,
    pub expected: Graph,
}

/// One edit to a document: replace a byte range.
#[derive(Clone, Debug)]
struct Edit {
    span: StdRange<usize>,
    replacement: String,
    /// For a link: the link as written and its range; for a definition: the definition.
    unit: StdRange<usize>,
    line: usize,
    reason: Option<String>,
    /// Whether the plan lists it as a rewritten link (heading markers and the like are not).
    show: bool,
}

/// Plans one operation.
pub fn plan(snapshot: &Snapshot, op: &Op, policy: &Policy) -> Result<Plan, PlanError> {
    let mut v = Vec::new();
    for issue in &snapshot.unreadable {
        v.push(Violation {
            invariant: "I2",
            path: issue.path.clone(),
            line: None,
            message: format!("{} — links in it cannot be checked", issue.message),
        });
    }
    if !v.is_empty() {
        return Err(PlanError::Refused(v));
    }
    match op {
        Op::Move { from, to } => plan_move(snapshot, from, to, policy),
        Op::Delete { path } => plan_delete(snapshot, path, policy),
        Op::Split { .. } | Op::Merge { .. } => restructure::plan(snapshot, op, policy),
    }
}

fn usage_path(path: &str, what: &str) -> Result<(), PlanError> {
    crate::path::check_inside_root(path)
        .map_err(|why| PlanError::Usage(format!("{what} {path:?} {why}")))
}

fn in_subtree(path: &str, root: &str) -> bool {
    path == root || (path.starts_with(root) && path[root.len()..].starts_with('/'))
}

fn lower(s: &str) -> String {
    crate::path::fold(s)
}

/// The planned listing: nodes after the move, the folders the move creates, and the folders
/// that the move leaves empty (to be removed).
struct Moved {
    mapping: HashMap<String, String>,
    nodes: Vec<Node>,
    remove_folders: Vec<String>,
    moves: Vec<Move>,
}

fn plan_move(snap: &Snapshot, from: &str, to: &str, policy: &Policy) -> Result<Plan, PlanError> {
    usage_path(from, "source")?;
    usage_path(to, "destination")?;
    let kinds: HashMap<&str, NodeKind> = snap
        .nodes
        .iter()
        .map(|n| (n.path.as_str(), n.kind))
        .collect();
    let Some(&from_kind) = kinds.get(from) else {
        return Err(PlanError::Usage(format!(
            "no such document or folder: {from}"
        )));
    };
    if from == to {
        return Ok(Plan {
            changes: empty_changes(snap),
            expected: snap.graph(),
        });
    }

    let mut violations = Vec::new();
    let mut refuse = |path: &str, message: &str| {
        violations.push(Violation {
            invariant: "I5",
            path: path.to_string(),
            line: None,
            message: message.to_string(),
        })
    };
    if from_kind == NodeKind::Collection && in_subtree(to, from) {
        refuse(to, "cannot move a folder into itself");
    }
    if to.split('/').any(|piece| piece.starts_with('.')) {
        refuse(
            to,
            "a name that starts with a dot is hidden and is never read",
        );
    }
    match from_kind {
        NodeKind::Document if !crate::port::is_document_path(to) => {
            refuse(to, "a document must keep the .md extension")
        }
        NodeKind::Asset if crate::port::is_document_path(to) => {
            refuse(to, "an asset cannot take the name of a document")
        }
        _ => {}
    }
    // A piece of the destination path that is a document or an asset is not a folder.
    let mut prefix = String::new();
    let pieces: Vec<&str> = to.split('/').collect();
    for piece in &pieces[..pieces.len() - 1] {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(piece);
        if let Some(kind) = kinds.get(prefix.as_str()) {
            if *kind != NodeKind::Collection && !in_subtree(&prefix, from) {
                refuse(&prefix, "is not a folder");
            }
        }
    }
    if !violations.is_empty() {
        return Err(PlanError::Refused(violations));
    }

    let moved = apply_move(snap, from, to, from_kind);

    // I5: no two nodes may share a name in one folder, case aside.
    let moved_paths: std::collections::HashSet<&str> =
        moved.mapping.values().map(String::as_str).collect();
    let mut taken: HashMap<(String, String), &str> = HashMap::new();
    let mut clashes: Vec<Violation> = Vec::new();
    for node in &moved.nodes {
        let key = (lower(dir_of(&node.path)), lower(name_of(&node.path)));
        if let Some(other) = taken.insert(key, node.path.as_str()) {
            let (culprit, existing) = if moved_paths.contains(node.path.as_str()) {
                (node.path.as_str(), other)
            } else {
                (other, node.path.as_str())
            };
            clashes.push(Violation {
                invariant: "I5",
                path: culprit.to_string(),
                line: None,
                message: if existing != culprit {
                    format!("clashes with {existing} (names that differ only in case)")
                } else {
                    "already exists".to_string()
                },
            });
        }
    }
    if !clashes.is_empty() {
        clashes.sort_by(|a, b| a.path.cmp(&b.path));
        clashes.dedup_by(|a, b| a.path == b.path);
        return Err(PlanError::Refused(clashes));
    }

    finish(
        snap,
        &moved,
        MoveKind::Moving { to: to.to_string() },
        policy,
    )
}

/// Works out where every node goes.
fn apply_move(snap: &Snapshot, from: &str, to: &str, from_kind: NodeKind) -> Moved {
    let mut mapping: HashMap<String, String> = HashMap::new();
    for node in &snap.nodes {
        if in_subtree(&node.path, from) {
            mapping.insert(
                node.path.clone(),
                format!("{to}{}", &node.path[from.len()..]),
            );
        }
    }
    let mut moves = vec![Move {
        from: from.to_string(),
        to: to.to_string(),
    }];
    // A folder that is renamed takes its folder document (`name/name.md`) along.
    if from_kind == NodeKind::Collection && name_of(from) != name_of(to) {
        let old_doc = format!("{from}/{}.md", name_of(from));
        if let std::collections::hash_map::Entry::Occupied(mut e) = mapping.entry(old_doc) {
            let new_doc = format!("{to}/{}.md", name_of(to));
            e.insert(new_doc.clone());
            moves.push(Move {
                from: format!("{to}/{}.md", name_of(from)),
                to: new_doc,
            });
        }
    }
    let mut nodes: Vec<Node> = snap
        .nodes
        .iter()
        .map(|n| Node {
            path: mapping
                .get(&n.path)
                .cloned()
                .unwrap_or_else(|| n.path.clone()),
            kind: n.kind,
            version: n.version.clone(),
        })
        .collect();
    let (nodes, remove_folders) = settle_folders(snap, &mut nodes, &mapping, Some(to));
    Moved {
        mapping,
        nodes,
        remove_folders,
        moves,
    }
}

/// Adds the folders a destination needs and takes out the ones that the change leaves empty.
fn settle_folders(
    snap: &Snapshot,
    nodes: &mut Vec<Node>,
    mapping: &HashMap<String, String>,
    to: Option<&str>,
) -> (Vec<Node>, Vec<String>) {
    let mut have: BTreeSet<String> = nodes.iter().map(|n| n.path.clone()).collect();
    let mapped_targets: std::collections::HashSet<&str> =
        mapping.values().map(String::as_str).collect();
    // Folders on the way to the destination.
    let mut dir = to.map(|t| dir_of(t).to_string()).unwrap_or_default();
    while !dir.is_empty() {
        if have.insert(dir.clone()) {
            nodes.push(Node {
                path: dir.clone(),
                kind: NodeKind::Collection,
                version: String::new(),
            });
        }
        dir = dir_of(&dir).to_string();
    }
    // Folders that had something in them before and have nothing now.
    let had_children: BTreeSet<String> = snap
        .nodes
        .iter()
        .filter(|n| !dir_of(&n.path).is_empty())
        .map(|n| dir_of(&n.path).to_string())
        .collect();
    let mut remove = Vec::new();
    loop {
        let with_children: BTreeSet<String> = nodes
            .iter()
            .filter(|n| !dir_of(&n.path).is_empty())
            .map(|n| dir_of(&n.path).to_string())
            .collect();
        let empty: Vec<String> = nodes
            .iter()
            .filter(|n| {
                n.kind == NodeKind::Collection
                    && !with_children.contains(&n.path)
                    && had_children.contains(&n.path)
                    && !mapped_targets.contains(n.path.as_str())
            })
            .map(|n| n.path.clone())
            .collect();
        if empty.is_empty() {
            break;
        }
        remove.extend(empty.iter().cloned());
        nodes.retain(|n| !empty.contains(&n.path));
    }
    remove.sort_by(|a, b| {
        b.matches('/')
            .count()
            .cmp(&a.matches('/').count())
            .then_with(|| a.cmp(b))
    });
    (nodes.clone(), remove)
}

enum MoveKind {
    Moving { to: String },
    Deleting { removed: BTreeSet<String> },
}

fn empty_changes(snap: &Snapshot) -> ChangeSet {
    ChangeSet {
        version: 1,
        nodes_fingerprint: nodes_fingerprint(&snap.nodes),
        reads: Vec::new(),
        moves: Vec::new(),
        creates: Vec::new(),
        writes: Vec::new(),
        deletes: Vec::new(),
        remove_folders: Vec::new(),
        rewrites: Vec::new(),
    }
}

/// The rewriting, the checks and the change set for a planned listing.
fn finish(
    snap: &Snapshot,
    moved: &Moved,
    kind: MoveKind,
    policy: &Policy,
) -> Result<Plan, PlanError> {
    let old_index = Index::new(&snap.nodes);
    let new_index = Index::new(&moved.nodes);
    let removed: BTreeSet<String> = match &kind {
        MoveKind::Deleting { removed } => removed.clone(),
        MoveKind::Moving { .. } => BTreeSet::new(),
    };
    let map = |p: &str| -> String {
        moved
            .mapping
            .get(p)
            .cloned()
            .unwrap_or_else(|| p.to_string())
    };

    let mut new_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut writes = Vec::new();
    let mut rewrites = Vec::new();
    let mut changed_bytes = 0usize;
    let mut delete_violations = Vec::new();

    for (path, bytes) in &snap.files {
        if removed.contains(path) {
            continue;
        }
        let (_, reading) = &snap.readings[path];
        let new_path = map(path);
        let mut edits: Vec<Edit> = Vec::new();
        let mut seen_defs: BTreeSet<String> = BTreeSet::new();

        for link in &reading.links {
            let old = old_index.resolve(path, link);
            // Links to something being deleted.
            if let (MoveKind::Deleting { .. }, Resolution::Target { path: t, .. }) = (&kind, &old) {
                if removed.contains(t) {
                    if !policy.break_links {
                        delete_violations.push(Violation {
                            invariant: "I2",
                            path: path.clone(),
                            line: Some(link.line),
                            message: format!("{} would break", link.text),
                        });
                    }
                    continue;
                }
            }
            let edit = plan_link_edit(
                path,
                &new_path,
                link,
                &old,
                &new_index,
                &map,
                bytes,
                reading,
                &mut seen_defs,
            );
            if let Some(e) = edit {
                edits.push(e);
            }
        }

        if edits.is_empty() {
            new_files.insert(new_path, bytes.clone());
            continue;
        }
        edits.sort_by_key(|e| e.span.start);
        edits.dedup_by(|b, a| a.span == b.span);
        let (content, spans) = apply_edits(bytes, &mut edits);
        if !content_preserved(bytes, &content, &edits, &spans) {
            return Err(PlanError::Refused(vec![Violation {
                invariant: "I1",
                path: path.clone(),
                line: None,
                message: "rewriting would change text outside the links".to_string(),
            }]));
        }
        for e in &edits {
            changed_bytes += e.span.len().max(e.replacement.len());
            let unit_start = e.unit.start;
            let before = String::from_utf8_lossy(&bytes[e.unit.clone()]).into_owned();
            let rel = e.span.start - unit_start..e.span.end - unit_start;
            let mut after = before.clone();
            after.replace_range(rel, &e.replacement);
            rewrites.push(Rewrite {
                path: new_path.clone(),
                line: e.line,
                before,
                after,
                reason: e.reason.clone(),
            });
        }
        writes.push(Write {
            path: new_path.clone(),
            content: String::from_utf8_lossy(&content).into_owned(),
            note: None,
        });
        new_files.insert(new_path, content);
    }
    if !delete_violations.is_empty() {
        delete_violations.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        return Err(PlanError::Refused(delete_violations));
    }
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    rewrites.sort_by(|a, b| (&a.path, a.line, &a.before).cmp(&(&b.path, b.line, &b.before)));

    // The deletions.
    let mut deletes = Vec::new();
    if let MoveKind::Deleting { removed } = &kind {
        for node in &snap.nodes {
            if removed.contains(&node.path) && node.kind != NodeKind::Collection {
                let lines = (node.kind == NodeKind::Document)
                    .then(|| snap.files.get(&node.path))
                    .flatten()
                    .map(|b| count_lines(b));
                deletes.push(Delete {
                    path: node.path.clone(),
                    kind: if node.kind == NodeKind::Document {
                        "document".to_string()
                    } else {
                        "asset".to_string()
                    },
                    lines,
                });
            }
        }
        deletes.sort_by(|a, b| a.path.cmp(&b.path));
    }

    // The checks: rebuild the graph of the planned state and compare.
    let expected = build_graph(&SnapshotStore {
        nodes: &moved.nodes,
        files: &new_files,
    });
    let old_graph = snap.graph();
    let mut violations = Vec::new();
    check_links(
        &old_graph,
        &expected,
        &map,
        &removed,
        policy,
        &mut violations,
    );

    // I4: budget.
    let root_path = match &kind {
        MoveKind::Moving { to } => to.clone(),
        MoveKind::Deleting { removed } => removed.iter().next().cloned().unwrap_or_default(),
    };
    let mut touched: BTreeSet<&str> = writes.iter().map(|w| w.path.as_str()).collect();
    touched.insert(root_path.as_str());
    if let Some(max) = policy.max_touched {
        if touched.len() > max {
            violations.push(Violation {
                invariant: "I4",
                path: root_path.clone(),
                line: None,
                message: format!("would touch {} nodes, the limit is {max}", touched.len()),
            });
        }
    }
    if let Some(max) = policy.max_changed_bytes {
        if changed_bytes > max {
            violations.push(Violation {
                invariant: "I4",
                path: root_path,
                line: None,
                message: format!(
                    "would rewrite {changed_bytes} bytes of links, the limit is {max}"
                ),
            });
        }
    }
    if !violations.is_empty() {
        return Err(PlanError::Refused(violations));
    }

    let mut reads: Vec<ReadVersion> = snap
        .nodes
        .iter()
        .filter(|n| {
            n.kind == NodeKind::Document
                || (n.kind == NodeKind::Asset
                    && (moved.mapping.contains_key(&n.path) || removed.contains(&n.path)))
        })
        .map(|n| ReadVersion {
            path: n.path.clone(),
            version: n.version.clone(),
        })
        .collect();
    reads.sort_by(|a, b| a.path.cmp(&b.path));
    let mut remove_folders = moved.remove_folders.clone();
    if let MoveKind::Deleting { removed } = &kind {
        for node in &snap.nodes {
            if node.kind == NodeKind::Collection && removed.contains(&node.path) {
                remove_folders.push(node.path.clone());
            }
        }
        remove_folders.sort_by(|a, b| {
            b.matches('/')
                .count()
                .cmp(&a.matches('/').count())
                .then_with(|| a.cmp(b))
        });
        remove_folders.dedup();
    }
    let changes = ChangeSet {
        version: 1,
        nodes_fingerprint: nodes_fingerprint(&snap.nodes),
        reads,
        moves: match &kind {
            MoveKind::Moving { .. } => moved.moves.clone(),
            MoveKind::Deleting { .. } => Vec::new(),
        },
        creates: Vec::new(),
        writes,
        deletes,
        remove_folders,
        rewrites,
    };
    Ok(Plan { changes, expected })
}

fn count_lines(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let n = bytes.iter().filter(|&&b| b == b'\n').count();
    if bytes.ends_with(b"\n") {
        n
    } else {
        n + 1
    }
}

/// What a link needs, if anything: the edit that keeps it meaning what it meant.
#[allow(clippy::too_many_arguments)]
fn plan_link_edit(
    old_path: &str,
    new_path: &str,
    link: &RawLink,
    old: &Resolution,
    new_index: &Index,
    map: &dyn Fn(&str) -> String,
    bytes: &[u8],
    reading: &Reading,
    seen_defs: &mut BTreeSet<String>,
) -> Option<Edit> {
    let to_text = |r: &crate::read::Range| r.start..r.end;
    match old {
        Resolution::Broken => None,
        Resolution::Outside => {
            // A relative link that leaves the root keeps pointing at the same place.
            if link.syntax != Syntax::Path || dir_of(old_path) == dir_of(new_path) {
                return None;
            }
            let dest = link
                .target
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>');
            let dest = dest.replace('\\', "/");
            if is_external(&dest) {
                return None;
            }
            let dest = dest.split('?').next().unwrap_or("");
            if dest.is_empty() {
                return None;
            }
            let (ups, pieces) = outside_location(dir_of(old_path), &percent_decode(dest))?;
            let new_dest = relative_to_outside(dir_of(new_path), ups, &pieces);
            path_edit(link, reading, &new_dest, None, bytes, seen_defs)
        }
        Resolution::Target {
            path: target, kind, ..
        } => {
            let expected = map(target);
            let now = new_index.resolve(new_path, link);
            if matches!(&now, Resolution::Target { path, .. } if *path == expected) {
                return None;
            }
            match link.syntax {
                Syntax::Path => {
                    let mut new_dest = relative_between(dir_of(new_path), &expected);
                    if *kind == NodeKind::Collection && link.target.ends_with('/') {
                        new_dest.push('/');
                    }
                    path_edit(link, reading, &new_dest, None, bytes, seen_defs)
                }
                Syntax::Wiki | Syntax::Embed => {
                    let span = link.target_span.as_ref().map(to_text)?;
                    let path_form = link.target.contains('/');
                    let with_ext = link.target.to_lowercase().ends_with(".md");
                    let new_name =
                        wiki_name(new_path, &expected, *kind, path_form, with_ext, new_index);
                    let reason = match (&now, path_form) {
                        (Resolution::Target { path, .. }, _) => {
                            format!("the name alone would now mean {path}")
                        }
                        (_, false) => "the name would no longer resolve".to_string(),
                        (_, true) => "the path no longer exists".to_string(),
                    };
                    Some(Edit {
                        span: span.clone(),
                        replacement: new_name,
                        unit: to_text(&link.range),
                        line: link.line,
                        reason: Some(reason),
                        show: true,
                    })
                }
            }
        }
    }
}

/// The edit of a path link's destination (in the link itself, or in its definition).
fn path_edit(
    link: &RawLink,
    reading: &Reading,
    new_dest: &str,
    reason: Option<String>,
    bytes: &[u8],
    seen_defs: &mut BTreeSet<String>,
) -> Option<Edit> {
    let to_text = |r: &crate::read::Range| r.start..r.end;
    let (span, angle, unit, line) = match &link.label {
        None => (
            link.target_span.as_ref().map(to_text)?,
            link.angle,
            to_text(&link.range),
            link.line,
        ),
        Some(label) => {
            let def = reading.defs.iter().find(|d| &d.label == label)?;
            if !seen_defs.insert(label.clone()) {
                return None;
            }
            (
                def.target_span.as_ref().map(to_text)?,
                def.angle,
                to_text(&def.range),
                def.line,
            )
        }
    };
    // A query (`?x=1`) is not part of the path and is kept.
    let raw = std::str::from_utf8(&bytes[span.clone()]).unwrap_or("");
    let end = raw.find('?').map_or(span.end, |q| span.start + q);
    Some(Edit {
        span: span.start..end,
        replacement: encode_destination(new_dest, angle),
        unit,
        line,
        reason,
        show: true,
    })
}

/// The shortest way to write a wiki link name that resolves to `target`.
fn wiki_name(
    from: &str,
    target: &str,
    kind: NodeKind,
    path_form: bool,
    with_ext: bool,
    index: &Index,
) -> String {
    let probe = |name: &str| -> bool {
        let link = RawLink {
            syntax: Syntax::Wiki,
            text: String::new(),
            target: name.to_string(),
            heading: None,
            alias: None,
            range: crate::read::Range { start: 0, end: 0 },
            line: 0,
            heading_span: None,
            target_span: None,
            angle: false,
            label: None,
        };
        matches!(index.resolve(from, &link), Resolution::Target { path, .. } if path == target)
    };
    let keep_ext = kind != NodeKind::Document || with_ext;
    let full = if keep_ext {
        target.to_string()
    } else {
        target.strip_suffix(".md").unwrap_or(target).to_string()
    };
    if !path_form {
        let short = if keep_ext {
            name_of(target).to_string()
        } else {
            stem_of(name_of(target)).to_string()
        };
        if probe(&short) {
            return short;
        }
    }
    // A document in the root has no `/` in its path; a leading `/` makes the name a path.
    if full.contains('/') {
        full
    } else {
        format!("/{full}")
    }
}

/// Applies edits (sorted, non-overlapping) to the original bytes. Returns the new bytes and,
/// for each edit, where its replacement sits in them.
///
/// A replacement is written in the Unicode normalization form of the text it replaces, so a
/// rewritten link keeps the form its writer used even when the file name on disk is in the other.
fn apply_edits(bytes: &[u8], edits: &mut [Edit]) -> (Vec<u8>, Vec<StdRange<usize>>) {
    for e in edits.iter_mut() {
        let was = String::from_utf8_lossy(&bytes[e.span.clone()]);
        e.replacement = crate::path::in_form_of(&was, &e.replacement);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut spans = Vec::new();
    let mut pos = 0;
    for e in edits {
        // The offset is already part of every span (it is added when a document is read).
        out.extend_from_slice(&bytes[pos..e.span.start]);
        let at = out.len();
        out.extend_from_slice(e.replacement.as_bytes());
        spans.push(at..out.len());
        pos = e.span.end;
    }
    out.extend_from_slice(&bytes[pos..]);
    (out, spans)
}

/// The text outside the edited spans is the same before and after.
fn content_preserved(old: &[u8], new: &[u8], edits: &[Edit], spans: &[StdRange<usize>]) -> bool {
    let mut old_pos = 0;
    let mut new_pos = 0;
    for (e, s) in edits.iter().zip(spans) {
        if old[old_pos..e.span.start] != new[new_pos..s.start] {
            return false;
        }
        old_pos = e.span.end;
        new_pos = s.end;
    }
    old[old_pos..] == new[new_pos..]
}

/// I2 and I3: every link that resolved still resolves to the same document, every link that was
/// broken is untouched, and nothing that was linked to loses all its links.
fn check_links(
    old: &Graph,
    new: &Graph,
    map: &dyn Fn(&str) -> String,
    removed: &BTreeSet<String>,
    policy: &Policy,
    out: &mut Vec<Violation>,
) {
    let mut old_by_doc: BTreeMap<&str, Vec<&crate::graph::Link>> = BTreeMap::new();
    for l in &old.links {
        old_by_doc.entry(l.from.as_str()).or_default().push(l);
    }
    let mut new_by_doc: BTreeMap<&str, Vec<&crate::graph::Link>> = BTreeMap::new();
    for l in &new.links {
        new_by_doc.entry(l.from.as_str()).or_default().push(l);
    }
    for (doc, links) in &old_by_doc {
        if removed.contains(*doc) {
            continue;
        }
        let new_doc = map(doc);
        let Some(after) = new_by_doc.get(new_doc.as_str()) else {
            out.push(Violation {
                invariant: "I2",
                path: new_doc,
                line: None,
                message: "lost its links".to_string(),
            });
            continue;
        };
        if after.len() != links.len() {
            out.push(Violation {
                invariant: "I2",
                path: new_doc,
                line: None,
                message: format!("had {} links, would have {}", links.len(), after.len()),
            });
            continue;
        }
        for (b, a) in links.iter().zip(after.iter()) {
            match (&b.to, &a.to) {
                (Some(t), _) if removed.contains(t) => {
                    if !policy.break_links {
                        out.push(Violation {
                            invariant: "I2",
                            path: (*doc).to_string(),
                            line: Some(b.line),
                            message: format!("{} would break", b.link),
                        });
                    }
                }
                (Some(t), Some(t2)) if map(t) == *t2 => {}
                (Some(_), _) => out.push(Violation {
                    invariant: "I2",
                    path: new_doc.clone(),
                    line: Some(a.line),
                    message: format!("{} would no longer reach the same document", a.link),
                }),
                (None, None) if b.link == a.link => {}
                (None, _) => out.push(Violation {
                    invariant: "I2",
                    path: new_doc.clone(),
                    line: Some(a.line),
                    message: format!("{} was broken and must stay as it was", b.link),
                }),
            }
        }
    }
    // I3: a document that others linked to is still linked to by the same documents.
    for (target, sources) in &old.backlinks {
        if removed.contains(target) {
            continue;
        }
        let new_target = map(target);
        let still: BTreeSet<&str> = new
            .backlinks
            .get(&new_target)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default();
        for s in sources {
            if removed.contains(s) {
                continue;
            }
            if !still.contains(map(s).as_str()) {
                out.push(Violation {
                    invariant: "I3",
                    path: new_target.clone(),
                    line: None,
                    message: format!("would no longer be linked from {}", map(s)),
                });
            }
        }
    }
}

fn plan_delete(snap: &Snapshot, path: &str, policy: &Policy) -> Result<Plan, PlanError> {
    usage_path(path, "path")?;
    if !snap.nodes.iter().any(|n| n.path == path) {
        return Err(PlanError::Usage(format!(
            "no such document or folder: {path}"
        )));
    }
    let removed: BTreeSet<String> = snap
        .nodes
        .iter()
        .filter(|n| in_subtree(&n.path, path))
        .map(|n| n.path.clone())
        .collect();
    let remaining: Vec<Node> = snap
        .nodes
        .iter()
        .filter(|n| !removed.contains(&n.path))
        .cloned()
        .collect();
    let mut nodes = remaining;
    let (nodes, remove_folders) = settle_folders(snap, &mut nodes, &HashMap::new(), None);
    let moved = Moved {
        mapping: HashMap::new(),
        nodes,
        remove_folders,
        moves: Vec::new(),
    };
    finish(snap, &moved, MoveKind::Deleting { removed }, policy)
}
