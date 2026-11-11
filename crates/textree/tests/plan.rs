//! Planning moves, renames and deletes over in-memory hierarchies.

use std::collections::BTreeMap;

use textree::{
    build_graph, plan, ChangeSet, MemoryStore, Op, Plan, PlanError, Policy, Snapshot, Violation,
};

type Files = BTreeMap<String, Vec<u8>>;

fn files(list: &[(&str, &str)]) -> Files {
    list.iter()
        .map(|(p, c)| (p.to_string(), c.as_bytes().to_vec()))
        .collect()
}

fn store(files: &Files) -> MemoryStore {
    let mut s = MemoryStore::new();
    for (p, c) in files {
        s = s.with_file(p, c);
    }
    s
}

fn try_plan(files: &Files, op: &Op, policy: &Policy) -> Result<Plan, PlanError> {
    plan(&Snapshot::read(&store(files)), op, policy)
}

fn mv(files: &Files, from: &str, to: &str) -> Plan {
    try_plan(
        files,
        &Op::Move {
            from: from.to_string(),
            to: to.to_string(),
        },
        &Policy::default(),
    )
    .unwrap_or_else(|e| panic!("move {from} -> {to} refused: {e:?}"))
}

fn del(files: &Files, path: &str, break_links: bool) -> Result<Plan, PlanError> {
    try_plan(
        files,
        &Op::Delete {
            path: path.to_string(),
        },
        &Policy {
            break_links,
            ..Policy::default()
        },
    )
}

/// Applies a change set to a map of files the way a store would.
fn apply(files: &Files, cs: &ChangeSet) -> Files {
    let mut out = files.clone();
    for m in &cs.moves {
        let keys: Vec<String> = out
            .keys()
            .filter(|k| **k == m.from || k.starts_with(&format!("{}/", m.from)))
            .cloned()
            .collect();
        for k in keys {
            let v = out.remove(&k).unwrap();
            out.insert(format!("{}{}", m.to, &k[m.from.len()..]), v);
        }
    }
    for w in &cs.writes {
        out.insert(w.path.clone(), w.content.clone().into_bytes());
    }
    for d in &cs.deletes {
        out.remove(&d.path);
    }
    out
}

fn text(files: &Files, path: &str) -> String {
    String::from_utf8(
        files
            .get(path)
            .unwrap_or_else(|| panic!("no {path}"))
            .clone(),
    )
    .unwrap()
}

fn refused(r: Result<Plan, PlanError>) -> Vec<Violation> {
    match r {
        Err(PlanError::Refused(v)) => v,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn paths(files: &Files) -> Vec<&str> {
    files.keys().map(String::as_str).collect()
}

const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 1, 2, 3];

// --- S2-1 ---------------------------------------------------------------------------------

fn soil_tree() -> Files {
    let mut f = files(&[
        ("index.md", "See [soil](garden/soil.md) and [[soil]].\n"),
        (
            "garden/soil.md",
            "# Soil\n\nBack to [home](../index.md) and ![beds](beds.png) and [[index]].\n",
        ),
    ]);
    f.insert("garden/beds.png".to_string(), PNG.to_vec());
    f
}

#[test]
fn moving_a_document_rewrites_only_the_links_that_need_it() {
    let before = soil_tree();
    let p = mv(&before, "garden/soil.md", "archive/soil.md");
    let after = apply(&before, &p.changes);
    assert_eq!(
        text(&after, "index.md"),
        "See [soil](archive/soil.md) and [[soil]].\n"
    );
    assert_eq!(
        text(&after, "archive/soil.md"),
        "# Soil\n\nBack to [home](../index.md) and ![beds](../garden/beds.png) and [[index]].\n"
    );
    assert_eq!(after["garden/beds.png"], PNG);
    assert!(!after.contains_key("garden/soil.md"));

    let shown: Vec<(&str, usize, &str, &str)> = p
        .changes
        .rewrites
        .iter()
        .map(|r| (r.path.as_str(), r.line, r.before.as_str(), r.after.as_str()))
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                "archive/soil.md",
                3,
                "![beds](beds.png)",
                "![beds](../garden/beds.png)"
            ),
            (
                "index.md",
                1,
                "[soil](garden/soil.md)",
                "[soil](archive/soil.md)"
            ),
        ]
    );
    assert!(p.changes.remove_folders.is_empty());
}

#[test]
fn moving_an_asset_rewrites_the_links_to_it() {
    let mut before = files(&[("garden/soil.md", "![beds](beds.png)\n")]);
    before.insert("garden/beds.png".to_string(), PNG.to_vec());
    let p = mv(&before, "garden/beds.png", "images/beds.png");
    let after = apply(&before, &p.changes);
    assert_eq!(
        text(&after, "garden/soil.md"),
        "![beds](../images/beds.png)\n"
    );
    assert_eq!(after["images/beds.png"], PNG);
    assert!(!after.contains_key("garden/beds.png"));
}

// --- S2-2 ---------------------------------------------------------------------------------

fn projects_tree() -> Files {
    files(&[
        (
            "projects/projects.md",
            "Garden: [[garden]] and [g](garden/garden.md) and [w](garden/watering.md#schedule).\n",
        ),
        (
            "projects/garden/garden.md",
            "# Garden\n\nSee [watering](watering.md) and [up](../projects.md).\n",
        ),
        (
            "projects/garden/watering.md",
            "# Watering\n\n## Schedule\n\nDaily.\n",
        ),
    ])
}

#[test]
fn renaming_a_folder_takes_its_folder_document_along() {
    let before = projects_tree();
    let p = mv(&before, "projects/garden", "projects/yard");
    let after = apply(&before, &p.changes);
    assert_eq!(
        paths(&after),
        vec![
            "projects/projects.md",
            "projects/yard/watering.md",
            "projects/yard/yard.md"
        ]
    );
    assert_eq!(
        text(&after, "projects/projects.md"),
        "Garden: [[yard]] and [g](yard/yard.md) and [w](yard/watering.md#schedule).\n"
    );
    assert_eq!(
        text(&after, "projects/yard/yard.md"),
        text(&before, "projects/garden/garden.md")
    );
    assert_eq!(
        text(&after, "projects/yard/watering.md"),
        text(&before, "projects/garden/watering.md")
    );
}

#[test]
fn moving_a_folder_without_renaming_it_keeps_the_folder_document_name() {
    let before = projects_tree();
    let p = mv(&before, "projects/garden", "archive/garden");
    let after = apply(&before, &p.changes);
    assert_eq!(
        text(&after, "projects/projects.md"),
        "Garden: [[garden]] and [g](../archive/garden/garden.md) and [w](../archive/garden/watering.md#schedule).\n"
    );
    assert_eq!(
        text(&after, "archive/garden/garden.md"),
        "# Garden\n\nSee [watering](watering.md) and [up](../../projects/projects.md).\n"
    );
    assert!(after.keys().all(|k| !k.starts_with("projects/garden")));
}

// --- S2-3 ---------------------------------------------------------------------------------

#[test]
fn a_moved_document_does_not_silently_take_over_a_name_link() {
    let before = files(&[
        ("x/d/note.md", "# Intro\n"),
        ("y/note.md", "y\n"),
        ("p/other.md", "Bystander: [[note]]\n"),
        (
            "x/link.md",
            "Mover: [[note]] [[note|the note]] [[note#Intro]] ![[note]]\n",
        ),
    ]);
    let p = mv(&before, "x/d/note.md", "x/note.md");
    let after = apply(&before, &p.changes);
    assert_eq!(text(&after, "p/other.md"), "Bystander: [[y/note]]\n");
    assert_eq!(text(&after, "x/link.md"), text(&before, "x/link.md"));
    assert_eq!(text(&after, "x/note.md"), "# Intro\n");
    assert_eq!(p.changes.remove_folders, vec!["x/d".to_string()]);
    assert_eq!(p.changes.rewrites.len(), 1);
    assert_eq!(
        p.changes.rewrites[0].reason.as_deref(),
        Some("the name alone would now mean x/note.md")
    );
    // The graph of the planned state resolves every link as before.
    let g = build_graph(&store(&after));
    assert!(g.broken.is_empty());
}

// --- S2-4 ---------------------------------------------------------------------------------

#[test]
fn a_rename_that_only_changes_case_is_allowed() {
    let before = files(&[("Note.md", "# N\n"), ("ref.md", "[a](Note.md) [[Note]]\n")]);
    let p = mv(&before, "Note.md", "note.md");
    let after = apply(&before, &p.changes);
    assert_eq!(paths(&after), vec!["note.md", "ref.md"]);
    assert_eq!(text(&after, "ref.md"), "[a](note.md) [[Note]]\n");
}

// --- S2-5 ---------------------------------------------------------------------------------

fn small_tree() -> Files {
    files(&[
        ("a.md", "A [b](b.md)\n"),
        ("b.md", "B\n"),
        ("d/one.md", "1\n"),
        ("d/two.md", "2\n"),
    ])
}

fn move_err(f: &Files, from: &str, to: &str) -> Result<Plan, PlanError> {
    try_plan(
        f,
        &Op::Move {
            from: from.into(),
            to: to.into(),
        },
        &Policy::default(),
    )
}

#[test]
fn refusals_name_the_invariant_and_the_path() {
    let f = small_tree();
    let v = refused(move_err(&f, "a.md", "b.md"));
    assert_eq!((v[0].invariant, v[0].path.as_str()), ("I5", "b.md"));
    assert!(v[0].message.contains("already exists"));

    let v = refused(move_err(&f, "a.md", "B.md"));
    assert_eq!((v[0].invariant, v[0].path.as_str()), ("I5", "B.md"));
    assert!(
        v[0].message.contains("differ only in case"),
        "{}",
        v[0].message
    );

    let v = refused(move_err(&f, "d", "d/sub"));
    assert_eq!(v[0].invariant, "I5");
    assert!(v[0].message.contains("into itself"));

    let v = refused(move_err(&f, "a.md", "a.txt"));
    assert!(v[0].message.contains(".md"));

    let v = refused(move_err(&f, "a.md", "b.md/inner.md"));
    assert!(v[0].message.contains("not a folder"));
}

#[test]
fn bad_requests_are_usage_errors_not_refusals() {
    let f = small_tree();
    for (from, to) in [
        ("nope.md", "z.md"),
        ("a.md", "../outside.md"),
        ("a.md", "/tmp/x.md"),
        ("a.md", "d/./x.md"),
        ("a.md", ""),
    ] {
        match move_err(&f, from, to) {
            Err(PlanError::Usage(_)) => {}
            other => panic!("{from} -> {to}: expected a usage error, got {other:?}"),
        }
    }
}

#[test]
fn moving_a_node_to_where_it_is_does_nothing() {
    let f = small_tree();
    let p = mv(&f, "a.md", "a.md");
    assert!(p.changes.is_empty());
}

#[test]
fn the_budget_limits_how_much_a_change_may_touch() {
    let mut list = vec![("t.md".to_string(), "t\n".to_string())];
    for i in 1..=8 {
        list.push((format!("l{i}.md"), "[t](t.md)\n".to_string()));
    }
    let f: Files = list.into_iter().map(|(p, c)| (p, c.into_bytes())).collect();
    let op = Op::Move {
        from: "t.md".into(),
        to: "sub/t.md".into(),
    };
    let tight = Policy {
        max_touched: Some(3),
        ..Policy::default()
    };
    let v = refused(try_plan(&f, &op, &tight));
    assert_eq!(v[0].invariant, "I4");
    assert!(v[0].message.contains("9"), "{}", v[0].message);
    let roomy = Policy {
        max_touched: Some(9),
        ..Policy::default()
    };
    assert!(try_plan(&f, &op, &roomy).is_ok());
}

// --- S2-6 ---------------------------------------------------------------------------------

fn delete_tree() -> Files {
    files(&[
        ("a.md", "See [b](b.md) and [[c]].\n"),
        ("b.md", "x\n"),
        ("c.md", "y\n"),
        ("lonely.md", "z\n"),
        ("sub/s1.md", "s\n"),
    ])
}

#[test]
fn deleting_a_document_nothing_links_to_is_fine() {
    let f = delete_tree();
    let p = del(&f, "lonely.md", false).unwrap();
    assert_eq!(p.changes.deletes.len(), 1);
    assert_eq!(p.changes.deletes[0].lines, Some(1));
    let after = apply(&f, &p.changes);
    assert!(!after.contains_key("lonely.md"));
}

#[test]
fn deleting_a_linked_document_is_refused_unless_allowed() {
    let f = delete_tree();
    let v = refused(del(&f, "b.md", false));
    assert_eq!(
        (v[0].invariant, v[0].path.as_str(), v[0].line),
        ("I2", "a.md", Some(1))
    );
    assert_eq!(v[0].message, "[b](b.md) would break");
    assert_eq!(
        v[0].to_string(),
        "refused [I2] a.md:1: [b](b.md) would break"
    );

    let p = del(&f, "b.md", true).unwrap();
    let after = apply(&f, &p.changes);
    assert!(!after.contains_key("b.md"));
    // The link that is left behind is not touched.
    assert_eq!(text(&after, "a.md"), text(&f, "a.md"));
}

#[test]
fn deleting_a_folder_removes_what_is_in_it_and_the_folder() {
    let f = delete_tree();
    let p = del(&f, "sub", false).unwrap();
    assert_eq!(
        p.changes
            .deletes
            .iter()
            .map(|d| d.path.as_str())
            .collect::<Vec<_>>(),
        vec!["sub/s1.md"]
    );
    assert_eq!(p.changes.remove_folders, vec!["sub".to_string()]);
}

// --- S2-9 ---------------------------------------------------------------------------------

#[test]
fn rewriting_keeps_every_link_form_and_every_other_byte() {
    let hub = "# Hub\n[a](old/note.md#part) ![img](old/pic.png) [b](<old/my note.md>) [c](old/my%20note.md \"Title\")\n[d][ref] [e][] [[old/note|alias]] `[x](old/note.md)`\n\n```\n[x](old/note.md)\n```\n\n[ref]: old/note.md \"Ref title\"\n[e]: old/note.md\n";
    let mut before = files(&[
        ("hub.md", hub),
        ("old/note.md", "x\n"),
        ("old/my note.md", "y\n"),
    ]);
    before.insert("crlf.md".into(), b"a\r\n[l](old/note.md)\r\n".to_vec());
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(b"[l](old/note.md)\n");
    before.insert("bom.md".into(), bom);
    before.insert("old/pic.png".into(), PNG.to_vec());

    let p = mv(&before, "old", "new");
    let after = apply(&before, &p.changes);
    assert_eq!(
        text(&after, "hub.md"),
        "# Hub\n[a](new/note.md#part) ![img](new/pic.png) [b](<new/my note.md>) [c](new/my%20note.md \"Title\")\n[d][ref] [e][] [[new/note|alias]] `[x](old/note.md)`\n\n```\n[x](old/note.md)\n```\n\n[ref]: new/note.md \"Ref title\"\n[e]: new/note.md\n"
    );
    assert_eq!(after["crlf.md"], b"a\r\n[l](new/note.md)\r\n");
    let mut want = vec![0xEF, 0xBB, 0xBF];
    want.extend_from_slice(b"[l](new/note.md)\n");
    assert_eq!(after["bom.md"], want);
    assert_eq!(after["new/pic.png"], PNG);
    assert!(after.keys().all(|k| !k.starts_with("old/")));
    assert!(build_graph(&store(&after)).broken.is_empty());
}

#[test]
fn a_link_that_leaves_the_root_keeps_pointing_at_the_same_place() {
    let before = files(&[("doc.md", "[o](../outside/z.md) [m](missing.md)\n")]);
    let p = mv(&before, "doc.md", "sub/doc.md");
    let after = apply(&before, &p.changes);
    // `[m]` was already broken and stays exactly as written.
    assert_eq!(
        text(&after, "sub/doc.md"),
        "[o](../../outside/z.md) [m](missing.md)\n"
    );
}

// --- S2-12 --------------------------------------------------------------------------------

#[test]
fn a_tag_starts_with_a_letter() {
    let f = files(&[(
        "t.md",
        "Issue #1577에 and #2는 and #123 and #_x and #태그 and #a1 and #2024-goals.\n",
    )]);
    let g = build_graph(&store(&f));
    assert_eq!(
        g.tags.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["_x", "a1", "태그"]
    );
}

#[test]
fn front_matter_tags_are_not_subject_to_the_body_rule() {
    let f = files(&[("fm.md", "---\ntags: [2024, plan]\n---\nBody.\n")]);
    let g = build_graph(&store(&f));
    assert_eq!(
        g.tags.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["2024", "plan"]
    );
}

#[test]
fn reference_links_cover_their_whole_text() {
    let f = files(&[
        (
            "r.md",
            "See [x][] and [y] and [z][w].\n\n[x]: n.md\n[y]: n.md\n[w]: n.md\n",
        ),
        ("n.md", "n\n"),
    ]);
    let g = build_graph(&store(&f));
    let src = text(&f, "r.md");
    let cut: Vec<&str> = g
        .links
        .iter()
        .filter(|l| l.from == "r.md")
        .map(|l| &src[l.range.start..l.range.end])
        .collect();
    assert_eq!(cut, vec!["[x][]", "[y]", "[z][w]"]);
    let shown: Vec<&str> = g
        .links
        .iter()
        .filter(|l| l.from == "r.md")
        .map(|l| l.link.as_str())
        .collect();
    assert_eq!(shown, vec!["[x][]", "[y]", "[z][w]"]);
}

// --- properties -----------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn random_tree(rng: &mut Rng) -> Files {
    let names = ["note", "idea", "plan", "log", "soil", "seed"];
    let dirs = ["", "a/", "a/b/", "c/", "c/d/", "e/"];
    let mut paths = Vec::new();
    while paths.len() < 14 {
        let p = format!(
            "{}{}.md",
            dirs[rng.below(dirs.len())],
            names[rng.below(names.len())]
        );
        if !paths.contains(&p) {
            paths.push(p);
        }
    }
    let mut out = Files::new();
    for p in &paths {
        let mut body = String::from("# Doc\n");
        for _ in 0..rng.below(5) {
            let t = &paths[rng.below(paths.len())];
            let stem = t.rsplit('/').next().unwrap().trim_end_matches(".md");
            let dir = p.rsplit_once('/').map_or("", |(d, _)| d);
            body.push_str(&match rng.below(5) {
                0 => format!("[[{stem}]] "),
                1 => format!("[[{}]] ", t.trim_end_matches(".md")),
                2 => format!("[[{stem}|alias]] "),
                3 => format!("[l]({}) ", textree::path::relative_between(dir, t)),
                _ => format!("[[{stem}#Doc]] "),
            });
        }
        body.push_str("[gone](missing.md)\n");
        out.insert(p.clone(), body.into_bytes());
    }
    out
}

#[test]
fn random_moves_never_break_or_retarget_a_link() {
    for seed in [3u64, 99, 2026, 77777, 123456789, 42, 8675309] {
        let mut rng = Rng(seed);
        let mut f = random_tree(&mut rng);
        for step in 0..40 {
            let g = build_graph(&store(&f));
            let docs: Vec<String> = g.documents.iter().map(|d| d.path.clone()).collect();
            // Now and then move a whole folder instead of one document.
            let folders: Vec<String> = {
                let mut set = std::collections::BTreeSet::new();
                for d in &docs {
                    let mut dir = d.rsplit_once('/').map(|(d, _)| d.to_string());
                    while let Some(x) = dir {
                        dir = x.rsplit_once('/').map(|(d, _)| d.to_string());
                        set.insert(x);
                    }
                }
                set.into_iter().collect()
            };
            let (from, to) = if !folders.is_empty() && rng.below(4) == 0 {
                let from = folders[rng.below(folders.len())].clone();
                let parents = ["", "a/", "c/", "z/", "z/y/"];
                let to = format!(
                    "{}{}",
                    parents[rng.below(parents.len())],
                    ["moved", "e", "b", "d", "q"][rng.below(5)]
                );
                (from, to)
            } else {
                let from = docs[rng.below(docs.len())].clone();
                let dirs = ["", "a/", "c/d/", "z/", "z/y/", "a/b/"];
                let to = format!(
                    "{}{}.md",
                    dirs[rng.below(dirs.len())],
                    ["note", "idea", "plan", "moved", "soil"][rng.below(5)]
                );
                (from, to)
            };
            match try_plan(
                &f,
                &Op::Move {
                    from: from.clone(),
                    to: to.clone(),
                },
                &Policy::default(),
            ) {
                Err(PlanError::Refused(v)) => {
                    assert!(
                        v.iter().all(|x| x.invariant == "I5"),
                        "seed {seed} step {step}: {from} -> {to}: {v:?}"
                    );
                }
                Err(PlanError::Usage(_)) => {}
                Ok(p) => {
                    let after = apply(&f, &p.changes);
                    let after_graph = build_graph(&store(&after));
                    let leaf = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
                    let rename = |x: &str| {
                        if x == from {
                            to.clone()
                        } else if x == format!("{from}/{}.md", leaf(&from))
                            && leaf(&from) != leaf(&to)
                        {
                            // A renamed folder takes its folder document along.
                            format!("{to}/{}.md", leaf(&to))
                        } else if let Some(rest) = x.strip_prefix(&format!("{from}/")) {
                            format!("{to}/{rest}")
                        } else {
                            x.to_string()
                        }
                    };
                    let mut want: Vec<(String, Option<String>)> = g
                        .links
                        .iter()
                        .map(|l| (rename(&l.from), l.to.as_deref().map(rename)))
                        .collect();
                    let mut got: Vec<(String, Option<String>)> = after_graph
                        .links
                        .iter()
                        .map(|l| (l.from.clone(), l.to.clone()))
                        .collect();
                    want.sort();
                    got.sort();
                    assert_eq!(want, got, "seed {seed} step {step}: {from} -> {to}");
                    // Broken links are byte-for-byte the same.
                    let broken = |g: &textree::Graph| {
                        let mut v: Vec<String> = g.broken.iter().map(|b| b.link.clone()).collect();
                        v.sort();
                        v
                    };
                    assert_eq!(broken(&g), broken(&after_graph), "seed {seed} step {step}");
                    f = after;
                }
            }
        }
    }
}
