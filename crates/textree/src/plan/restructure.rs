//! Planning a split and a merge.
//!
//! A split turns one document into a folder document and one new document per section; a merge
//! turns several documents into sections of one. Both move text between documents, so a link
//! can no longer be followed by its path alone: it has to be followed by its path *and* its
//! heading fragment. This module generalises "where does this link go" to that pair, rewrites
//! every link, same-document anchor and reference definition whose meaning would change, and
//! checks the planned state by rebuilding its relation graph.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range as StdRange;

use super::*;
use crate::anchors::{anchors_of, file_stem_of, github_slug};
use crate::graph::Link;
use crate::path::{fold, nfc};
use crate::read::{Footnote, Heading, RefDef};

/// What happens to the heading fragment of a link.
#[derive(Clone, Debug)]
enum Frag {
    Keep,
    /// Replace the fragment of a path link (an anchor).
    Set(String),
    /// A link without a fragment gets one: (anchor for a path link, heading text for a wiki link).
    Add(String, String),
}

#[derive(Clone, Debug)]
struct Expected {
    path: String,
    frag: Frag,
    reason: Option<String>,
}

type ExpectFn<'a> = dyn Fn(&str, Option<&str>) -> Expected + 'a;

struct Ctx<'a> {
    old_index: &'a Index,
    new_index: &'a Index,
    expect: &'a ExpectFn<'a>,
}

pub(super) fn plan(snap: &Snapshot, op: &Op, policy: &Policy) -> Result<Plan, PlanError> {
    match op {
        Op::Split { path, level } => split(snap, path, *level, policy),
        Op::Merge { into, from } => merge(snap, into, from, policy),
        _ => Err(PlanError::Usage(
            "not a restructuring operation".to_string(),
        )),
    }
}

// ---- small helpers ---------------------------------------------------------------------------

fn refuse(invariant: &'static str, path: &str, line: Option<usize>, message: String) -> PlanError {
    PlanError::Refused(vec![Violation {
        invariant,
        path: path.to_string(),
        line,
        message,
    }])
}

fn line_ending_of(bytes: &[u8]) -> &'static str {
    match bytes.iter().position(|&b| b == b'\n') {
        Some(i) if i > 0 && bytes[i - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

fn line_start(bytes: &[u8], pos: usize) -> usize {
    bytes[..pos.min(bytes.len())]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1)
}

/// The text with trailing blank lines dropped and exactly one line ending at the end; an empty
/// text stays empty.
fn tidy(text: &str, le: &str) -> String {
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out: String = lines.concat();
    if !out.ends_with('\n') {
        out.push_str(le);
    }
    out
}

fn replace(span: StdRange<usize>, text: String, unit: StdRange<usize>, line: usize) -> Edit {
    Edit {
        span,
        replacement: text,
        unit,
        line,
        reason: None,
        show: true,
    }
}

fn quiet(span: StdRange<usize>, text: String) -> Edit {
    Edit {
        unit: span.clone(),
        span,
        replacement: text,
        line: 0,
        reason: None,
        show: false,
    }
}

fn def_link(d: &RefDef) -> RawLink {
    RawLink {
        syntax: Syntax::Path,
        text: String::new(),
        target: d.target.clone(),
        heading: d.heading.clone(),
        alias: None,
        range: d.range,
        line: d.line,
        heading_span: d.heading_span,
        target_span: d.target_span,
        angle: d.angle,
        label: None,
    }
}

fn span_of(r: &crate::read::Range) -> StdRange<usize> {
    r.start..r.end
}

/// Whether a fragment, as written in a link, already names the heading that has this anchor.
fn same_anchor(fragment: &str, anchor: &str) -> bool {
    let (fragment, anchor) = (nfc(fragment), nfc(anchor));
    fragment.to_lowercase() == anchor || github_slug(&fragment) == anchor
}

/// Which heading of a document a fragment names: the first whose anchor is the fragment, or
/// whose anchor is the slug of the fragment (a wiki link writes the heading text).
pub(super) fn heading_for(fragment: &str, anchors: &[String]) -> Option<usize> {
    let fragment = nfc(fragment);
    let lower = fragment.to_lowercase();
    let slug = github_slug(&fragment);
    anchors
        .iter()
        .position(|a| nfc(a) == lower || nfc(a) == slug)
}

// ---- editing the links of one document -------------------------------------------------------

/// The edits that keep one link meaning what it meant, as seen from `new_from`.
fn link_edits(ctx: &Ctx<'_>, old_from: &str, new_from: &str, link: &RawLink, out: &mut Vec<Edit>) {
    let old = ctx.old_index.resolve(old_from, link);
    let unit = span_of(&link.range);
    match &old {
        Resolution::Broken => {}
        Resolution::Outside => {
            if link.syntax != Syntax::Path || dir_of(old_from) == dir_of(new_from) {
                return;
            }
            let dest = link
                .target
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>');
            let dest = dest.replace('\\', "/");
            if is_external(&dest) {
                return;
            }
            let dest = dest.split('?').next().unwrap_or("");
            if dest.is_empty() {
                return;
            }
            let Some((ups, pieces)) = outside_location(dir_of(old_from), &percent_decode(dest))
            else {
                return;
            };
            if let Some(ts) = &link.target_span {
                let new_dest = relative_to_outside(dir_of(new_from), ups, &pieces);
                out.push(replace(
                    span_of(ts),
                    encode_destination(&new_dest, link.angle),
                    unit,
                    link.line,
                ));
            }
        }
        Resolution::Target {
            path: target, kind, ..
        } => {
            let exp = (ctx.expect)(target, link.heading.as_deref());
            let now = ctx.new_index.resolve(new_from, link);
            let path_ok = matches!(&now, Resolution::Target { path, .. } if *path == exp.path);
            match link.syntax {
                Syntax::Path => {
                    if let Some(ts) = &link.target_span {
                        if !path_ok {
                            let mut dest = relative_between(dir_of(new_from), &exp.path);
                            if *kind == NodeKind::Collection && link.target.ends_with('/') {
                                dest.push('/');
                            }
                            out.push(replace(
                                span_of(ts),
                                encode_destination(&dest, link.angle),
                                unit.clone(),
                                link.line,
                            ));
                        }
                        match &exp.frag {
                            Frag::Keep => {}
                            Frag::Set(f) => {
                                if let Some(hs) = &link.heading_span {
                                    out.push(replace(span_of(hs), f.clone(), unit, link.line));
                                }
                            }
                            Frag::Add(f, _) => {
                                if link.heading_span.is_none() {
                                    out.push(replace(
                                        ts.end..ts.end,
                                        format!("#{f}"),
                                        unit,
                                        link.line,
                                    ));
                                }
                            }
                        }
                    }
                }
                Syntax::Wiki | Syntax::Embed => {
                    let Some(ts) = &link.target_span else { return };
                    if !path_ok {
                        let path_form = link.target.contains('/');
                        let with_ext = link.target.to_lowercase().ends_with(".md");
                        let name = wiki_name(
                            new_from,
                            &exp.path,
                            *kind,
                            path_form,
                            with_ext,
                            ctx.new_index,
                        );
                        let reason =
                            exp.reason
                                .clone()
                                .unwrap_or_else(|| match (&now, path_form) {
                                    (Resolution::Target { path, .. }, _) => {
                                        format!("the name alone would now mean {path}")
                                    }
                                    (_, false) => "the name would no longer resolve".to_string(),
                                    (_, true) => "the path no longer exists".to_string(),
                                });
                        let mut e = replace(span_of(ts), name, unit.clone(), link.line);
                        e.reason = Some(reason);
                        out.push(e);
                    }
                    match &exp.frag {
                        Frag::Add(_, heading) if link.heading_span.is_none() => {
                            out.push(replace(
                                ts.end..ts.end,
                                format!("#{heading}"),
                                unit,
                                link.line,
                            ));
                        }
                        // The heading is named by its anchor, which tells apart headings
                        // that read the same.
                        Frag::Set(anchor) => {
                            if let Some(hs) = &link.heading_span {
                                out.push(replace(span_of(hs), anchor.clone(), unit, link.line));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Every link and reference definition of a document. `new_from` says which document a
/// position of the old text ends up in.
fn doc_edits(
    ctx: &Ctx<'_>,
    old_from: &str,
    new_from: &dyn Fn(usize) -> String,
    reading: &Reading,
) -> Vec<Edit> {
    let mut out = Vec::new();
    for link in &reading.links {
        // A reference link takes its destination from its definition, which is edited itself.
        if link.label.is_some() {
            continue;
        }
        link_edits(ctx, old_from, &new_from(link.range.start), link, &mut out);
    }
    for def in &reading.defs {
        link_edits(
            ctx,
            old_from,
            &new_from(def.range.start),
            &def_link(def),
            &mut out,
        );
    }
    out
}

/// The text of a document after the edits, and where each old position ended up.
fn apply(bytes: &[u8], edits: &mut Vec<Edit>) -> (Vec<u8>, Box<dyn Fn(usize) -> usize>) {
    edits.sort_by_key(|e| (e.span.start, e.span.end));
    edits.dedup_by(|b, a| a.span == b.span && a.replacement == b.replacement);
    let (content, _) = apply_edits(bytes, edits);
    let snapshot: Vec<(usize, usize, isize)> = edits
        .iter()
        .map(|e| {
            (
                e.span.start,
                e.span.end,
                e.replacement.len() as isize - e.span.len() as isize,
            )
        })
        .collect();
    let map = move |p: usize| -> usize {
        let mut delta = 0isize;
        for (s, e, d) in &snapshot {
            if *e <= p && *s < p {
                delta += d;
            }
        }
        (p as isize + delta) as usize
    };
    (content, Box::new(map))
}

fn push_rewrites(out: &mut Vec<Rewrite>, new_path: &str, bytes: &[u8], edits: &[Edit]) -> usize {
    let mut changed = 0;
    let mut units: Vec<StdRange<usize>> = Vec::new();
    for e in edits.iter().filter(|e| e.show) {
        if !units.contains(&e.unit) {
            units.push(e.unit.clone());
        }
    }
    units.sort_by_key(|u| (u.start, u.end));
    for unit in units {
        let mut group: Vec<&Edit> = edits.iter().filter(|e| e.show && e.unit == unit).collect();
        group.sort_by_key(|e| std::cmp::Reverse((e.span.start, e.span.end)));
        let before = String::from_utf8_lossy(&bytes[unit.clone()]).into_owned();
        let mut after = before.clone();
        let mut reason = None;
        let mut line = 0;
        for e in &group {
            changed += e.span.len().max(e.replacement.len());
            let rel = e.span.start - unit.start..e.span.end - unit.start;
            after.replace_range(rel, &e.replacement);
            reason = reason.or_else(|| e.reason.clone());
            line = e.line;
        }
        out.push(Rewrite {
            path: new_path.to_string(),
            line,
            before,
            after,
            reason,
        });
    }
    changed
}

/// The marker edits that move a heading to a new level (an underlined heading becomes `#`s).
fn heading_edits(h: &Heading, new_level: usize, bytes: &[u8], out: &mut Vec<Edit>) {
    if h.nested || new_level == h.level as usize {
        return;
    }
    let marker = "#".repeat(new_level);
    if !h.setext {
        out.push(quiet(
            h.range.start..h.range.start + h.level as usize,
            marker,
        ));
    } else {
        out.push(quiet(h.range.start..h.range.start, format!("{marker} ")));
        let end = h.range.end.min(bytes.len());
        if h.title_end < end {
            out.push(quiet(h.title_end..end, String::new()));
        }
    }
}

fn footnote_problem(
    footnotes: &[(String, &Footnote)],
    part_of: &dyn Fn(usize) -> usize,
) -> Option<(usize, String)> {
    let mut defs: HashMap<&str, usize> = HashMap::new();
    for (_, f) in footnotes.iter().filter(|(_, f)| f.definition) {
        defs.insert(f.label.as_str(), part_of(f.range.start));
    }
    for (_, f) in footnotes.iter().filter(|(_, f)| !f.definition) {
        if let Some(&q) = defs.get(f.label.as_str()) {
            if q != part_of(f.range.start) {
                return Some((
                    f.line,
                    format!("footnote [^{}] is defined in another section", f.label),
                ));
            }
        }
    }
    None
}

/// Links per document of a graph, in text order.
fn links_by_doc(g: &Graph) -> BTreeMap<&str, Vec<&Link>> {
    let mut m: BTreeMap<&str, Vec<&Link>> = BTreeMap::new();
    for l in &g.links {
        m.entry(l.from.as_str()).or_default().push(l);
    }
    m
}

/// Number of links with a heading fragment that names a heading of their target.
fn valid_fragments(g: &Graph) -> usize {
    let anchors: HashMap<&str, Vec<String>> = g
        .documents
        .iter()
        .map(|d| {
            let titles: Vec<&str> = d.headings.iter().map(|h| h.text.as_str()).collect();
            (d.path.as_str(), anchors_of(&titles))
        })
        .collect();
    g.links
        .iter()
        .filter(|l| match (&l.heading, &l.to) {
            (Some(f), Some(t)) => anchors
                .get(t.as_str())
                .is_some_and(|a| heading_for(f, a).is_some()),
            _ => false,
        })
        .count()
}

/// For a document that is cut up or put together: the links it had must all still be there,
/// reaching the same documents, plus exactly the `extra` ones the operation adds; links that
/// were broken stay as they were.
fn check_bag(
    old: &[&Link],
    new: &[&Link],
    extra: usize,
    path: &str,
    expect: &ExpectFn<'_>,
    out: &mut Vec<Violation>,
) {
    let mut bad = |message: String| {
        out.push(Violation {
            invariant: "I2",
            path: path.to_string(),
            line: None,
            message,
        });
    };
    if new.len() != old.len() + extra {
        bad(format!(
            "had {} links, would have {} (expected {})",
            old.len(),
            new.len(),
            old.len() + extra
        ));
        return;
    }
    let mut want: BTreeMap<String, usize> = BTreeMap::new();
    let mut broken: Vec<&str> = Vec::new();
    for l in old {
        match &l.to {
            Some(t) => {
                *want
                    .entry(expect(t, l.heading.as_deref()).path)
                    .or_default() += 1
            }
            None => broken.push(&l.link),
        }
    }
    let mut got: BTreeMap<String, usize> = BTreeMap::new();
    let mut now_broken: Vec<&str> = Vec::new();
    for l in new {
        match &l.to {
            Some(t) => *got.entry(t.clone()).or_default() += 1,
            None => now_broken.push(&l.link),
        }
    }
    for (t, n) in &want {
        if got.get(t).copied().unwrap_or(0) < *n {
            bad(format!("a link to {t} would no longer reach it"));
        }
    }
    broken.sort_unstable();
    now_broken.sort_unstable();
    if broken != now_broken {
        bad("a broken link would change".to_string());
    }
}

/// Pairs the links of the old state with those of the new and checks that each still goes where
/// it was expected to go, and that broken links are untouched.
fn check_pairs(
    old: &[&Link],
    new: &[&Link],
    new_path: &str,
    expect: &ExpectFn<'_>,
    out: &mut Vec<Violation>,
) {
    if old.len() != new.len() {
        out.push(Violation {
            invariant: "I2",
            path: new_path.to_string(),
            line: None,
            message: format!("had {} links, would have {}", old.len(), new.len()),
        });
        return;
    }
    for (b, a) in old.iter().zip(new.iter()) {
        match &b.to {
            Some(t) => {
                let want = expect(t, b.heading.as_deref()).path;
                if a.to.as_deref() != Some(want.as_str()) {
                    out.push(Violation {
                        invariant: "I2",
                        path: new_path.to_string(),
                        line: Some(a.line),
                        message: format!("{} would no longer reach the same document", a.link),
                    });
                }
            }
            None => {
                if a.to.is_some() || a.link != b.link {
                    out.push(Violation {
                        invariant: "I2",
                        path: new_path.to_string(),
                        line: Some(a.line),
                        message: format!("{} was broken and must stay as it was", b.link),
                    });
                }
            }
        }
    }
}

// ---- split -------------------------------------------------------------------------------------

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn split(snap: &Snapshot, path: &str, level: u8, policy: &Policy) -> Result<Plan, PlanError> {
    crate::path::check_inside_root(path)
        .map_err(|why| PlanError::Usage(format!("path {path:?} {why}")))?;
    let Some(node) = snap.nodes.iter().find(|n| n.path == path) else {
        return Err(PlanError::Usage(format!(
            "no such document or folder: {path}"
        )));
    };
    if node.kind != NodeKind::Document {
        return Err(PlanError::Usage(format!("not a document: {path}")));
    }
    if !(1..=6).contains(&level) {
        return Err(PlanError::Usage(format!(
            "--level must be 1 to 6, not {level}"
        )));
    }
    let bytes = &snap.files[path];
    let (_, reading) = &snap.readings[path];
    let heads = &reading.headings;
    let split_idx: Vec<usize> = heads
        .iter()
        .enumerate()
        .filter(|(_, h)| h.level == level && !h.nested)
        .map(|(i, _)| i)
        .collect();
    if split_idx.is_empty() {
        return Err(PlanError::Usage(format!(
            "no level-{level} heading to split at in {path}"
        )));
    }
    for h in &heads[split_idx[0] + 1..] {
        if !h.nested && h.level < level {
            return Err(refuse(
                "I1",
                path,
                Some(h.line),
                format!("a level-{} heading follows the first split point", h.level),
            ));
        }
    }

    // Where the parts start (at the start of the heading's line) and where they go.
    let starts: Vec<usize> = split_idx
        .iter()
        .map(|&i| line_start(bytes, heads[i].range.start))
        .collect();
    let part_of = |pos: usize| starts.partition_point(|s| *s <= pos);
    let parts = starts.len() + 1;

    let dir = dir_of(path);
    let stem = stem_of(name_of(path)).to_string();
    let folder_doc = !dir.is_empty() && name_of(dir) == stem;
    let (bundle_path, child_dir) = if folder_doc {
        (path.to_string(), dir.to_string())
    } else {
        let cd = join(dir, &stem);
        (format!("{cd}/{}", name_of(path)), cd)
    };
    if !folder_doc && snap.nodes.iter().any(|n| fold(&n.path) == fold(&child_dir)) {
        return Err(refuse("I5", &child_dir, None, "already exists".to_string()));
    }

    // Names of the new documents.
    let mut taken: HashSet<String> = HashSet::new();
    taken.insert(fold(name_of(&bundle_path)));
    if folder_doc {
        for n in &snap.nodes {
            if dir_of(&n.path) == child_dir {
                taken.insert(fold(name_of(&n.path)));
            }
        }
    }
    let mut part_path: Vec<String> = vec![bundle_path.clone()];
    for k in 1..parts {
        let base = file_stem_of(&nfc(&heads[split_idx[k - 1]].text))
            .unwrap_or_else(|| format!("section-{k}"));
        let mut name = base.clone();
        let mut n = 2;
        while taken.contains(&fold(&format!("{name}.md"))) {
            name = format!("{base}-{n}");
            n += 1;
        }
        taken.insert(fold(&format!("{name}.md")));
        part_path.push(join(&child_dir, &format!("{name}.md")));
    }

    // Anchors before and after.
    let titles: Vec<&str> = heads.iter().map(|h| h.text.as_str()).collect();
    let old_anchors = anchors_of(&titles);
    let head_part: Vec<usize> = heads.iter().map(|h| part_of(h.range.start)).collect();
    let mut new_anchor: Vec<String> = vec![String::new(); heads.len()];
    for q in 0..parts {
        let idx: Vec<usize> = (0..heads.len()).filter(|&j| head_part[j] == q).collect();
        let ts: Vec<&str> = idx.iter().map(|&j| titles[j]).collect();
        for (j, a) in idx.iter().zip(anchors_of(&ts)) {
            new_anchor[*j] = a;
        }
    }

    // Footnotes may not cross sections.
    let notes: Vec<(String, &Footnote)> = reading
        .footnotes
        .iter()
        .map(|f| (path.to_string(), f))
        .collect();
    if let Some((line, message)) = footnote_problem(&notes, &part_of) {
        return Err(refuse("I2", path, Some(line), message));
    }

    // The new hierarchy.
    let mut nodes: Vec<Node> = snap
        .nodes
        .iter()
        .filter(|n| folder_doc || n.path != path)
        .cloned()
        .collect();
    let mut have: BTreeSet<String> = nodes.iter().map(|n| n.path.clone()).collect();
    let mut d = child_dir.clone();
    while !d.is_empty() {
        if have.insert(d.clone()) {
            nodes.push(Node {
                path: d.clone(),
                kind: NodeKind::Collection,
                version: String::new(),
            });
        }
        d = dir_of(&d).to_string();
    }
    for (k, p) in part_path.iter().enumerate() {
        if (k == 0 && folder_doc) || !have.insert(p.clone()) {
            continue;
        }
        nodes.push(Node {
            path: p.clone(),
            kind: NodeKind::Document,
            version: String::new(),
        });
    }
    let old_index = Index::new(&snap.nodes);
    let new_index = Index::new(&nodes);

    let src = path.to_string();
    let part_path_c = part_path.clone();
    let old_anchors_c = old_anchors.clone();
    let new_anchor_c = new_anchor.clone();
    let head_part_c = head_part.clone();
    let expect = move |t: &str, heading: Option<&str>| -> Expected {
        if t != src {
            return Expected {
                path: t.to_string(),
                frag: Frag::Keep,
                reason: None,
            };
        }
        if let Some(f) = heading {
            if let Some(j) = heading_for(f, &old_anchors_c) {
                let q = head_part_c[j];
                let frag = if same_anchor(f, &new_anchor_c[j]) {
                    Frag::Keep
                } else {
                    Frag::Set(new_anchor_c[j].clone())
                };
                return Expected {
                    path: part_path_c[q].clone(),
                    frag,
                    reason: Some(format!("the section moved to {}", part_path_c[q])),
                };
            }
        }
        Expected {
            path: part_path_c[0].clone(),
            frag: Frag::Keep,
            reason: None,
        }
    };
    let ctx = Ctx {
        old_index: &old_index,
        new_index: &new_index,
        expect: &expect,
    };

    // Edits inside the split document.
    let le = line_ending_of(bytes);
    let pp = part_path.clone();
    let new_from = move |pos: usize| pp[part_of(pos)].clone();
    let mut edits = doc_edits(&ctx, path, &new_from, reading);
    // Same-document anchors that now point into another document.
    for a in &reading.anchors {
        let p = part_of(a.range.start);
        let Some(f) = a.heading.as_deref() else {
            continue;
        };
        let Some(j) = heading_for(f, &old_anchors) else {
            continue;
        };
        let q = head_part[j];
        let anchor_changed = !same_anchor(f, &new_anchor[j]);
        let unit = span_of(&a.range);
        if q == p {
            if anchor_changed {
                if let Some(hs) = &a.heading_span {
                    edits.push(replace(span_of(hs), new_anchor[j].clone(), unit, a.line));
                }
            }
            continue;
        }
        let Some(ts) = &a.target_span else { continue };
        match a.syntax {
            Syntax::Path => {
                let rel = relative_between(dir_of(&part_path[p]), &part_path[q]);
                edits.push(replace(
                    span_of(ts),
                    encode_destination(&rel, a.angle),
                    unit.clone(),
                    a.line,
                ));
                if anchor_changed {
                    if let Some(hs) = &a.heading_span {
                        edits.push(replace(span_of(hs), new_anchor[j].clone(), unit, a.line));
                    }
                }
            }
            _ => {
                let name = wiki_name(
                    &part_path[p],
                    &part_path[q],
                    NodeKind::Document,
                    false,
                    false,
                    &new_index,
                );
                let mut e = replace(span_of(ts), name, unit.clone(), a.line);
                e.reason = Some(format!("the section moved to {}", part_path[q]));
                edits.push(e);
                if anchor_changed {
                    if let Some(hs) = &a.heading_span {
                        edits.push(replace(span_of(hs), new_anchor[j].clone(), unit, a.line));
                    }
                }
            }
        }
    }
    // Heading levels move up with the sections.
    for (j, h) in heads.iter().enumerate() {
        if head_part[j] >= 1 {
            let new_level = (h.level as usize).saturating_sub(level as usize - 1).max(1);
            heading_edits(h, new_level, bytes, &mut edits);
        }
    }
    // Definitions that a section uses but that sit in another section are copied into it.
    let mut needs: Vec<Vec<usize>> = vec![Vec::new(); parts];
    for l in &reading.links {
        if let Some(label) = &l.label {
            if let Some(di) = reading.defs.iter().position(|d| &d.label == label) {
                let p = part_of(l.range.start);
                if part_of(reading.defs[di].range.start) != p && !needs[p].contains(&di) {
                    needs[p].push(di);
                }
            }
        }
    }
    let (edited, map) = apply(bytes, &mut edits);
    let mut changed_bytes = 0;
    let mut rewrites = Vec::new();
    let mut text_of: Vec<String> = Vec::with_capacity(parts);
    for q in 0..parts {
        let from = starts.get(q.wrapping_sub(1)).copied().unwrap_or(0);
        let to = starts.get(q).copied().unwrap_or(bytes.len());
        let a = if q == 0 { 0 } else { map(from) };
        let b = if q == parts - 1 {
            edited.len()
        } else {
            map(to)
        };
        let slice = String::from_utf8_lossy(&edited[a..b]).into_owned();
        text_of.push(tidy(&slice, le));
    }
    for q in 0..parts {
        for &di in &needs[q] {
            let d = &reading.defs[di];
            let mut copy_edits = Vec::new();
            link_edits(&ctx, path, &part_path[q], &def_link(d), &mut copy_edits);
            let mut def_text = bytes[d.range.start..d.range.end].to_vec();
            for e in &mut copy_edits {
                e.span = e.span.start - d.range.start..e.span.end - d.range.start;
            }
            copy_edits.sort_by_key(|e| e.span.start);
            let (copied, _) = apply_edits(&def_text, &mut copy_edits);
            def_text = copied;
            let t = String::from_utf8_lossy(&def_text).trim_end().to_string();
            if !text_of[q].is_empty() {
                text_of[q].push_str(le);
            }
            text_of[q].push_str(&t);
            text_of[q].push_str(le);
        }
    }
    // The folder document: what came before the first split point, then a list of the sections.
    let mut list = String::new();
    for k in 1..parts {
        let name = wiki_name(
            &part_path[0],
            &part_path[k],
            NodeKind::Document,
            false,
            false,
            &new_index,
        );
        list.push_str(&format!("- [[{name}]]{le}"));
    }
    let mut bundle = text_of[0].clone();
    if bundle.is_empty() {
        bundle = list;
    } else {
        // Keep the copied definitions at the end, after the list.
        let defs_start = bundle.len();
        let _ = defs_start;
        bundle.push_str(le);
        bundle.push_str(&list);
    }
    text_of[0] = bundle;

    // Doc edits' rewrite entries, for the split document, appear under their new documents.
    let mut link_rewrites: Vec<(String, Vec<Edit>)> = Vec::new();
    {
        let mut by_part: BTreeMap<usize, Vec<Edit>> = BTreeMap::new();
        for e in edits.iter().filter(|e| e.show) {
            by_part
                .entry(part_of(e.unit.start))
                .or_default()
                .push(e.clone());
        }
        for (q, es) in by_part {
            link_rewrites.push((part_path[q].clone(), es));
        }
    }
    for (p, es) in &link_rewrites {
        changed_bytes += push_rewrites(&mut rewrites, p, bytes, es);
    }

    // Everything else in the hierarchy.
    let mut new_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut writes: Vec<Write> = Vec::new();
    for (p, b) in &snap.files {
        if p == path {
            continue;
        }
        let (_, rd) = &snap.readings[p];
        let me = p.clone();
        let nf = move |_: usize| me.clone();
        let mut es = doc_edits(&ctx, p, &nf, rd);
        if es.is_empty() {
            new_files.insert(p.clone(), b.clone());
            continue;
        }
        es.sort_by_key(|e| (e.span.start, e.span.end));
        es.dedup_by(|x, y| x.span == y.span && x.replacement == y.replacement);
        let (content, _) = apply_edits(b, &mut es);
        changed_bytes += push_rewrites(&mut rewrites, p, b, &es);
        writes.push(Write {
            path: p.clone(),
            content: String::from_utf8_lossy(&content).into_owned(),
            note: None,
        });
        new_files.insert(p.clone(), content);
    }
    let note_first = format!("\"{}\"", heading_line(bytes, &heads[split_idx[0]]));
    let mut creates: Vec<Write> = Vec::new();
    for (q, text) in text_of.iter().enumerate() {
        let content = text.clone().into_bytes();
        let mut content_bytes = content;
        if q == 0
            && bytes.starts_with(&[0xEF, 0xBB, 0xBF])
            && !content_bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(&content_bytes);
            content_bytes = v;
        }
        new_files.insert(part_path[q].clone(), content_bytes.clone());
        let s = String::from_utf8_lossy(&content_bytes).into_owned();
        if q == 0 {
            let has_pre = !tidy(&String::from_utf8_lossy(&edited[..map(starts[0])]), le).is_empty();
            let n = parts - 1;
            let list = format!(
                "a list of {}",
                if n == 1 {
                    "1 link".to_string()
                } else {
                    format!("{n} links")
                }
            );
            writes.push(Write {
                path: bundle_path.clone(),
                content: s,
                note: Some(if has_pre {
                    format!("text before {note_first} + {list}")
                } else {
                    list
                }),
            });
        } else {
            let h = &heads[split_idx[q - 1]];
            creates.push(Write {
                path: part_path[q].clone(),
                content: s,
                note: Some(format!(
                    "from \"{}\", {}",
                    heading_line(bytes, h),
                    plural_lines(count_lines(&content_bytes))
                )),
            });
        }
    }
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    creates.sort_by(|a, b| a.path.cmp(&b.path));
    rewrites.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));

    // The checks.
    let expected = build_graph(&SnapshotStore {
        nodes: &nodes,
        files: &new_files,
    });
    let old_graph = snap.graph();
    let mut violations = Vec::new();
    let old_by = links_by_doc(&old_graph);
    let new_by = links_by_doc(&expected);
    for (doc, olds) in &old_by {
        if *doc == path {
            continue;
        }
        let news = new_by.get(doc).cloned().unwrap_or_default();
        check_pairs(olds, &news, doc, &expect, &mut violations);
    }
    {
        let olds = old_by.get(path).cloned().unwrap_or_default();
        let mut news: Vec<&Link> = Vec::new();
        for (q, p) in part_path.iter().enumerate() {
            let mut ls = new_by.get(p.as_str()).cloned().unwrap_or_default();
            if q == 0 {
                let n = ls.len().saturating_sub(parts - 1);
                ls.truncate(n);
            }
            news.extend(ls);
        }
        let converted = reading
            .anchors
            .iter()
            .filter(|a| {
                a.heading
                    .as_deref()
                    .and_then(|f| heading_for(f, &old_anchors))
                    .is_some_and(|j| head_part[j] != part_of(a.range.start))
            })
            .count();
        check_bag(&olds, &news, converted, path, &expect, &mut violations);
    }
    if valid_fragments(&expected) < valid_fragments(&old_graph) {
        violations.push(Violation {
            invariant: "I2",
            path: path.to_string(),
            line: None,
            message: "a link to a heading would lose its heading".to_string(),
        });
    }
    let touched: BTreeSet<&str> = writes
        .iter()
        .chain(creates.iter())
        .map(|w| w.path.as_str())
        .collect();
    if let Some(max) = policy.max_touched {
        if touched.len() > max {
            violations.push(Violation {
                invariant: "I4",
                path: path.to_string(),
                line: None,
                message: format!("would touch {} nodes, the limit is {max}", touched.len()),
            });
        }
    }
    if let Some(max) = policy.max_changed_bytes {
        if changed_bytes > max {
            violations.push(Violation {
                invariant: "I4",
                path: path.to_string(),
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
        .filter(|n| n.kind == NodeKind::Document)
        .map(|n| ReadVersion {
            path: n.path.clone(),
            version: n.version.clone(),
        })
        .collect();
    reads.sort_by(|a, b| a.path.cmp(&b.path));
    let moves = if folder_doc {
        Vec::new()
    } else {
        vec![Move {
            from: path.to_string(),
            to: bundle_path.clone(),
        }]
    };
    let changes = ChangeSet {
        version: 2,
        ops: Vec::new(),
        policy: Policy::default(),
        nodes_fingerprint: nodes_fingerprint(&snap.nodes),
        reads,
        moves,
        creates,
        writes,
        deletes: Vec::new(),
        remove_folders: Vec::new(),
        rewrites,
        new_broken: Vec::new(),
        orphaned: Vec::new(),
    };
    Ok(Plan { changes, expected })
}

fn plural_lines(n: usize) -> String {
    format!("{n} {}", if n == 1 { "line" } else { "lines" })
}

/// The heading as it is written on its first line, for the plan.
fn heading_line(bytes: &[u8], h: &Heading) -> String {
    let title = String::from_utf8_lossy(&bytes[h.title_start..h.title_end.min(bytes.len())]);
    if h.setext {
        title.trim().to_string()
    } else {
        format!("{} {}", "#".repeat(h.level as usize), title.trim())
    }
}

// ---- merge -------------------------------------------------------------------------------------

fn merge(snap: &Snapshot, into: &str, from: &[String], policy: &Policy) -> Result<Plan, PlanError> {
    for p in std::iter::once(into).chain(from.iter().map(String::as_str)) {
        crate::path::check_inside_root(p)
            .map_err(|why| PlanError::Usage(format!("path {p:?} {why}")))?;
        match snap.nodes.iter().find(|n| n.path == p) {
            None => return Err(PlanError::Usage(format!("no such document: {p}"))),
            Some(n) if n.kind != NodeKind::Document => {
                return Err(PlanError::Usage(format!("not a document: {p}")));
            }
            _ => {}
        }
    }
    if from.is_empty() {
        return Err(PlanError::Usage(
            "nothing to merge into the document".to_string(),
        ));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    seen.insert(into);
    for f in from {
        if !seen.insert(f.as_str()) {
            return Err(PlanError::Usage(format!(
                "{f} is named more than once (or is the target)"
            )));
        }
    }

    // What cannot be merged.
    for s in from {
        let (_, rd) = &snap.readings[s];
        if rd.front_matter.is_some() {
            return Err(refuse(
                "I1",
                s,
                None,
                "has front matter, which would be lost".to_string(),
            ));
        }
        for h in &rd.headings {
            if !h.nested && h.level >= 6 {
                return Err(refuse(
                    "I1",
                    s,
                    Some(h.line),
                    "heading would be deeper than level 6".to_string(),
                ));
            }
        }
    }
    let docs: Vec<&str> = std::iter::once(into)
        .chain(from.iter().map(String::as_str))
        .collect();
    // Footnotes and reference definitions of different documents must not meet.
    let mut fn_def: HashMap<String, &str> = HashMap::new();
    for d in &docs {
        for f in snap.readings[*d]
            .1
            .footnotes
            .iter()
            .filter(|f| f.definition)
        {
            if let Some(first) = fn_def.get(&f.label) {
                return Err(refuse(
                    "I2",
                    d,
                    Some(f.line),
                    format!("footnote [^{}] is also defined in {first}", f.label),
                ));
            }
            fn_def.insert(f.label.clone(), d);
        }
    }
    for d in &docs {
        for f in snap.readings[*d]
            .1
            .footnotes
            .iter()
            .filter(|f| !f.definition)
        {
            if let Some(owner) = fn_def.get(&f.label) {
                if owner != d {
                    return Err(refuse(
                        "I2",
                        d,
                        Some(f.line),
                        format!("footnote [^{}] is defined in {owner}", f.label),
                    ));
                }
            }
        }
    }
    let mut ref_def: HashMap<String, (&str, String)> = HashMap::new();
    for d in &docs {
        for rd in &snap.readings[*d].1.defs {
            let dest = format!("{}#{}", rd.target, rd.heading.clone().unwrap_or_default());
            match ref_def.get(&rd.label) {
                Some((first, first_dest)) if first != d && *first_dest != dest => {
                    return Err(refuse(
                        "I2",
                        d,
                        Some(rd.line),
                        format!("reference [{}] is defined differently in {first}", rd.label),
                    ));
                }
                None => {
                    ref_def.insert(rd.label.clone(), (d, dest));
                }
                _ => {}
            }
        }
    }

    // The sections: a title for each source, and the anchors of the merged document.
    struct Section {
        starts_with_h1: bool,
        title: String,
    }
    let mut sections: Vec<Section> = Vec::new();
    for s in from {
        let (_, rd) = &snap.readings[s];
        let bytes = &snap.files[s];
        let first = rd.headings.first();
        let starts_with_h1 = first.is_some_and(|h| {
            let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
            let bom = bytes.len() - body.len();
            h.level == 1
                && !h.nested
                && h.range.start >= bom
                && body[..h.range.start - bom]
                    .iter()
                    .all(|b| b.is_ascii_whitespace())
        });
        let title = if starts_with_h1 {
            first.map(|h| h.text.clone()).unwrap_or_default()
        } else {
            stem_of(name_of(s)).to_string()
        };
        sections.push(Section {
            starts_with_h1,
            title,
        });
    }
    let into_heads = &snap.readings[into].1.headings;
    let mut all_titles: Vec<String> = into_heads.iter().map(|h| h.text.clone()).collect();
    // Index of each source heading (and of its synthesized title) in the merged document.
    let mut source_first: Vec<usize> = Vec::new();
    let mut synth_at: Vec<Option<usize>> = Vec::new();
    for (i, s) in from.iter().enumerate() {
        if sections[i].starts_with_h1 {
            synth_at.push(None);
        } else {
            synth_at.push(Some(all_titles.len()));
            all_titles.push(sections[i].title.clone());
        }
        source_first.push(all_titles.len());
        for h in &snap.readings[s].1.headings {
            all_titles.push(h.text.clone());
        }
    }
    let merged_anchors = anchors_of(&all_titles);
    let section_anchor = |i: usize| -> String {
        match synth_at[i] {
            Some(k) => merged_anchors[k].clone(),
            None => merged_anchors[source_first[i]].clone(),
        }
    };
    let old_anchor_of: Vec<Vec<String>> = from
        .iter()
        .map(|s| {
            let t: Vec<&str> = snap.readings[s]
                .1
                .headings
                .iter()
                .map(|h| h.text.as_str())
                .collect();
            anchors_of(&t)
        })
        .collect();

    // The new hierarchy.
    let mut nodes: Vec<Node> = snap
        .nodes
        .iter()
        .filter(|n| !from.contains(&n.path))
        .cloned()
        .collect();
    let (nodes, remove_folders) = settle_folders(snap, &mut nodes, &HashMap::new(), None);
    let old_index = Index::new(&snap.nodes);
    let new_index = Index::new(&nodes);

    let sources: Vec<String> = from.to_vec();
    let into_s = into.to_string();
    let anchors_by_source = old_anchor_of.clone();
    let section_anchors: Vec<String> = (0..from.len()).map(section_anchor).collect();
    let source_headings: Vec<Vec<usize>> = from
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (0..snap.readings[s].1.headings.len())
                .map(|j| source_first[i] + j)
                .collect()
        })
        .collect();
    let merged_anchors_c = merged_anchors.clone();
    // How a wiki link names a section: by its title, unless an earlier heading of the merged
    // document reads the same, in which case by its anchor.
    let titles_c: Vec<String> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let at = synth_at[i].unwrap_or(source_first[i]);
            if heading_for(&s.title, &merged_anchors) == Some(at) {
                s.title.clone()
            } else {
                merged_anchors[at].clone()
            }
        })
        .collect();
    let expect = move |t: &str, heading: Option<&str>| -> Expected {
        if let Some(i) = sources.iter().position(|s| s == t) {
            let reason = Some(format!("{t} was merged into {into_s}"));
            let frag = match heading {
                Some(f) => match heading_for(f, &anchors_by_source[i]) {
                    Some(j) => {
                        let a = merged_anchors_c[source_headings[i][j]].clone();
                        if same_anchor(f, &a) {
                            Frag::Keep
                        } else {
                            Frag::Set(a)
                        }
                    }
                    None => Frag::Keep,
                },
                None => Frag::Add(section_anchors[i].clone(), titles_c[i].clone()),
            };
            return Expected {
                path: into_s.clone(),
                frag,
                reason,
            };
        }
        Expected {
            path: t.to_string(),
            frag: Frag::Keep,
            reason: None,
        }
    };
    let ctx = Ctx {
        old_index: &old_index,
        new_index: &new_index,
        expect: &expect,
    };

    let into_bytes = &snap.files[into];
    let le = line_ending_of(into_bytes);
    let mut new_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut writes: Vec<Write> = Vec::new();
    let mut rewrites: Vec<Rewrite> = Vec::new();
    let mut changed_bytes = 0;

    // The merged document: the target, then one section per source.
    let into_rd = &snap.readings[into].1;
    let into_name = into.to_string();
    let nf = move |_: usize| into_name.clone();
    let mut into_edits = doc_edits(&ctx, into, &nf, into_rd);
    let (into_edited, _) = apply(into_bytes, &mut into_edits);
    changed_bytes += push_rewrites(&mut rewrites, into, into_bytes, &into_edits);
    let mut merged = tidy(&String::from_utf8_lossy(&into_edited), le);
    let mut note_parts: Vec<String> = Vec::new();
    for (i, s) in from.iter().enumerate() {
        let bytes = &snap.files[s];
        let rd = &snap.readings[s].1;
        let into_name = into.to_string();
        let nf = move |_: usize| into_name.clone();
        let mut edits = doc_edits(&ctx, s, &nf, rd);
        // Same-document anchors follow the renumbering of anchors.
        for a in &rd.anchors {
            let Some(f) = a.heading.as_deref() else {
                continue;
            };
            if let (Some(j), Some(hs)) = (heading_for(f, &old_anchor_of[i]), &a.heading_span) {
                let na = &merged_anchors[source_headings_idx(source_first[i], j)];
                if !same_anchor(f, na) {
                    edits.push(replace(span_of(hs), na.clone(), span_of(&a.range), a.line));
                }
            }
        }
        for h in &rd.headings {
            heading_edits(h, h.level as usize + 1, bytes, &mut edits);
        }
        let (edited, _) = apply(bytes, &mut edits);
        changed_bytes += push_rewrites(&mut rewrites, into, bytes, &edits);
        let mut body: &[u8] = &edited;
        if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
            body = &body[3..];
        }
        let mut text = String::from_utf8_lossy(body).into_owned();
        text = text.trim_start_matches(['\r', '\n']).to_string();
        // The lines of a source keep the line ending they had.
        let sle = line_ending_of(bytes);
        if !sections[i].starts_with_h1 {
            text = format!("## {}{sle}{sle}{text}", sections[i].title);
        }
        merged.push_str(le);
        merged.push_str(&tidy(&text, sle));
        note_parts.push(format!("{s} as section \"## {}\"", sections[i].title));
    }
    let mut merged_bytes = merged.into_bytes();
    if into_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) && !merged_bytes.starts_with(&[0xEF, 0xBB, 0xBF])
    {
        let mut v = vec![0xEF, 0xBB, 0xBF];
        v.extend_from_slice(&merged_bytes);
        merged_bytes = v;
    }
    writes.push(Write {
        path: into.to_string(),
        content: String::from_utf8_lossy(&merged_bytes).into_owned(),
        note: Some(format!("adds {}", note_parts.join(", "))),
    });
    new_files.insert(into.to_string(), merged_bytes);

    // Everything else.
    for (p, b) in &snap.files {
        if p == into || from.contains(p) {
            continue;
        }
        let (_, rd) = &snap.readings[p];
        let me = p.clone();
        let nf = move |_: usize| me.clone();
        let mut es = doc_edits(&ctx, p, &nf, rd);
        if es.is_empty() {
            new_files.insert(p.clone(), b.clone());
            continue;
        }
        let (content, _) = apply(b, &mut es);
        changed_bytes += push_rewrites(&mut rewrites, p, b, &es);
        writes.push(Write {
            path: p.clone(),
            content: String::from_utf8_lossy(&content).into_owned(),
            note: None,
        });
        new_files.insert(p.clone(), content);
    }
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    rewrites.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let mut deletes: Vec<Delete> = from
        .iter()
        .map(|s| Delete {
            path: s.clone(),
            kind: "document".to_string(),
            lines: Some(count_lines(&snap.files[s])),
        })
        .collect();
    deletes.sort_by(|a, b| a.path.cmp(&b.path));

    // The checks.
    let expected = build_graph(&SnapshotStore {
        nodes: &nodes,
        files: &new_files,
    });
    let old_graph = snap.graph();
    let mut violations = Vec::new();
    let old_by = links_by_doc(&old_graph);
    let new_by = links_by_doc(&expected);
    for (doc, olds) in &old_by {
        if *doc == into || from.iter().any(|f| f == doc) {
            continue;
        }
        let news = new_by.get(doc).cloned().unwrap_or_default();
        check_pairs(olds, &news, doc, &expect, &mut violations);
    }
    {
        let mut olds: Vec<&Link> = old_by.get(into).cloned().unwrap_or_default();
        for s in from {
            olds.extend(old_by.get(s.as_str()).cloned().unwrap_or_default());
        }
        let news = new_by.get(into).cloned().unwrap_or_default();
        check_bag(&olds, &news, 0, into, &expect, &mut violations);
    }
    if valid_fragments(&expected) < valid_fragments(&old_graph) {
        violations.push(Violation {
            invariant: "I2",
            path: into.to_string(),
            line: None,
            message: "a link to a heading would lose its heading".to_string(),
        });
    }
    let touched: BTreeSet<&str> = writes
        .iter()
        .map(|w| w.path.as_str())
        .chain(from.iter().map(String::as_str))
        .collect();
    if let Some(max) = policy.max_touched {
        if touched.len() > max {
            violations.push(Violation {
                invariant: "I4",
                path: into.to_string(),
                line: None,
                message: format!("would touch {} nodes, the limit is {max}", touched.len()),
            });
        }
    }
    if let Some(max) = policy.max_changed_bytes {
        if changed_bytes > max {
            violations.push(Violation {
                invariant: "I4",
                path: into.to_string(),
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
        .filter(|n| n.kind == NodeKind::Document)
        .map(|n| ReadVersion {
            path: n.path.clone(),
            version: n.version.clone(),
        })
        .collect();
    reads.sort_by(|a, b| a.path.cmp(&b.path));
    let changes = ChangeSet {
        version: 2,
        ops: Vec::new(),
        policy: Policy::default(),
        nodes_fingerprint: nodes_fingerprint(&snap.nodes),
        reads,
        moves: Vec::new(),
        creates: Vec::new(),
        writes,
        deletes,
        remove_folders,
        rewrites,
        new_broken: Vec::new(),
        orphaned: Vec::new(),
    };
    Ok(Plan { changes, expected })
}

fn source_headings_idx(first: usize, j: usize) -> usize {
    first + j
}
