//! `textree split` and `textree merge`, run as a user would run them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn with(files: &[(&str, &[u8])]) -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("textree-split-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (path, body) in files {
            let full = dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
        }
        Scratch(dir)
    }

    fn tree(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel, fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.0, &self.0, &mut out);
        out
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out: Output = Command::new(env!("CARGO_BIN_EXE_textree"))
            .arg(args[0])
            .arg(&self.0)
            .args(&args[1..])
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// The tree must be exactly this.
    fn assert_tree(&self, want: &[(&str, &[u8])]) {
        let want: BTreeMap<String, Vec<u8>> = want
            .iter()
            .map(|(p, b)| (p.to_string(), b.to_vec()))
            .collect();
        let got = self.tree();
        for k in got.keys().chain(want.keys()) {
            assert_eq!(
                got.get(k).map(|b| String::from_utf8_lossy(b).into_owned()),
                want.get(k).map(|b| String::from_utf8_lossy(b).into_owned()),
                "{k}"
            );
        }
        assert_eq!(got, want);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

type Files<'a> = &'a [(&'a str, &'a [u8])];

const TRIP: &[u8] = b"# Trip\n\nIntro text.\n\n## Day 1\n\nWalk.\n\n## Day 2\n\nRest.\n";

#[test]
fn split_shows_the_new_documents_and_writes_nothing_without_apply() {
    let s = Scratch::with(&[("notes/trip.md", TRIP)]);
    let (code, out, _) = s.run(&["split", "notes/trip.md"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "move     notes/trip.md -> notes/trip/trip.md\n\
         write    notes/trip/trip.md (text before \"## Day 1\" + a list of 2 links)\n\
         create   notes/trip/day-1.md (from \"## Day 1\", 3 lines)\n\
         create   notes/trip/day-2.md (from \"## Day 2\", 3 lines)\n\
         \n\
         checks   nothing disappears · links intact · no name clashes\n\
         Dry run: nothing was written. Use --apply to write.\n"
    );
    s.assert_tree(&[("notes/trip.md", TRIP)]);
}

#[test]
fn split_makes_a_folder_document_with_a_list_and_one_document_per_section() {
    let s = Scratch::with(&[("notes/trip.md", TRIP)]);
    let (code, out, _) = s.run(&["split", "notes/trip.md", "--apply"]);
    assert_eq!(code, 0);
    assert!(out.lines().last().unwrap().starts_with("Applied"));
    s.assert_tree(&[
        (
            "notes/trip/trip.md",
            b"# Trip\n\nIntro text.\n\n- [[day-1]]\n- [[day-2]]\n",
        ),
        ("notes/trip/day-1.md", b"# Day 1\n\nWalk.\n"),
        ("notes/trip/day-2.md", b"# Day 2\n\nRest.\n"),
    ]);
    let (code, out, _) = s.run(&["check"]);
    assert_eq!((code, out.trim()), (0, "no broken links"));
}

#[test]
fn split_keeps_every_link_meaning_what_it_meant() {
    let png: Vec<u8> = (0..=255u8).collect();
    let s = Scratch::with(&[
        (
            "index.md",
            b"Plan: [trip](notes/trip.md), [day 2](notes/trip.md#day-2), [[trip]], [[trip#Day 1]], [[trip#Trip]].\n",
        ),
        (
            "notes/trip.md",
            b"# Trip\n\nPacking: [list](../packing.md) and [[packing]].\n\n## Day 1\n\nWalk with ![map](map.png) and [[packing|the list]].\n\n## Day 2\n\nRest. See [[sleep]].\n",
        ),
        ("packing.md", b"p\n"),
        ("notes/map.png", &png),
        ("notes/sleep.md", b"z\n"),
        ("w/day-1.md", b"w\n"),
        ("notes/user.md", b"[[day-1]]\n"),
    ]);
    let (code, _, err) = s.run(&["split", "notes/trip.md", "--apply"]);
    assert_eq!(code, 0, "{err}");
    s.assert_tree(&[
        (
            "notes/trip/trip.md",
            b"# Trip\n\nPacking: [list](../../packing.md) and [[packing]].\n\n- [[day-1]]\n- [[day-2]]\n",
        ),
        (
            "notes/trip/day-1.md",
            b"# Day 1\n\nWalk with ![map](../map.png) and [[packing|the list]].\n",
        ),
        ("notes/trip/day-2.md", b"# Day 2\n\nRest. See [[sleep]].\n"),
        (
            "index.md",
            b"Plan: [trip](notes/trip/trip.md), [day 2](notes/trip/day-2.md#day-2), [[trip]], [[notes/trip/day-1#Day 1]], [[trip#Trip]].\n",
        ),
        ("notes/user.md", b"[[w/day-1]]\n"),
        ("packing.md", b"p\n"),
        ("notes/map.png", &png),
        ("notes/sleep.md", b"z\n"),
        ("w/day-1.md", b"w\n"),
    ]);
    assert_eq!(s.run(&["check"]).0, 0);
}

#[test]
fn split_follows_same_document_anchors_into_the_new_documents() {
    let s = Scratch::with(&[(
        "toc.md",
        b"# Guide\n\nContents: [One](#one), [Two](#two), [[#Two]].\n\n## One\n\nText. Back to [contents](#guide).\n\n## Two\n\nMore. See [One](#one).\n",
    )]);
    assert_eq!(s.run(&["split", "toc.md", "--apply"]).0, 0);
    s.assert_tree(&[
        (
            "toc/toc.md",
            b"# Guide\n\nContents: [One](one.md#one), [Two](two.md#two), [[two#Two]].\n\n- [[one]]\n- [[two]]\n",
        ),
        (
            "toc/one.md",
            b"# One\n\nText. Back to [contents](toc.md#guide).\n",
        ),
        ("toc/two.md", b"# Two\n\nMore. See [One](one.md#one).\n"),
    ]);
}

#[test]
fn split_copies_a_reference_definition_into_each_section_that_uses_it() {
    let s = Scratch::with(&[(
        "r.md",
        b"# R\n\n## A\n\nSee [x][ref].\n\n## B\n\nAlso [y][ref].\n\n[ref]: https://example.com\n",
    )]);
    assert_eq!(s.run(&["split", "r.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("r/r.md", b"# R\n\n- [[a]]\n- [[b]]\n"),
        (
            "r/a.md",
            b"# A\n\nSee [x][ref].\n\n[ref]: https://example.com\n",
        ),
        (
            "r/b.md",
            b"# B\n\nAlso [y][ref].\n\n[ref]: https://example.com\n",
        ),
    ]);
}

#[test]
fn split_refuses_what_it_cannot_do_safely_and_leaves_the_folder_alone() {
    let cases: &[(Files, &str, i32, &str)] = &[
        (
            &[(
                "f.md",
                b"# F\n\n## One\n\nText[^1].\n\n## Two\n\n[^1]: note\n",
            )],
            "f.md",
            1,
            "refused [I2] f.md:5:",
        ),
        (
            &[("h.md", b"# H\n\n## One\n\nx\n\n# Again\n\ny\n")],
            "h.md",
            1,
            "refused [I1] h.md:7:",
        ),
        (
            &[("n.md", b"# N\n\ntext\n")],
            "n.md",
            2,
            "no level-2 heading to split at",
        ),
        (
            &[("d.md", b"# D\n\n## A\n\na\n"), ("d/x.md", b"x\n")],
            "d.md",
            1,
            "refused [I5] d: already exists",
        ),
    ];
    for (files, path, code, message) in cases {
        let s = Scratch::with(files);
        let before = s.tree();
        let (got, _, err) = s.run(&["split", path, "--apply"]);
        assert_eq!(got, *code, "{path}: {err}");
        assert!(err.contains(message), "{path}: {err}");
        assert_eq!(s.tree(), before, "{path}");
    }
}

#[test]
fn split_does_not_take_a_heading_inside_a_quote_or_a_list_for_a_split_point() {
    let s = Scratch::with(&[(
        "q.md",
        b"# Q\n\n## Real\n\n> ## Quoted\n\n- ## Listed\n\ntext\n",
    )]);
    assert_eq!(s.run(&["split", "q.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("q/q.md", b"# Q\n\n- [[real]]\n"),
        (
            "q/real.md",
            b"# Real\n\n> ## Quoted\n\n- ## Listed\n\ntext\n",
        ),
    ]);
}

#[test]
fn split_names_the_new_documents_after_their_headings() {
    let doc = "# Doc\n\n## Day 1\n\na\n\n## Café & Bar!\n\nb\n\n## 한글 제목\n\nc\n\n## `code` and *emph* [link](https://example.com)\n\nd\n\n## Day 1\n\ne\n\n## !!!\n\nf\n\n## Doc\n\ng\n";
    let s = Scratch::with(&[("doc.md", doc.as_bytes())]);
    assert_eq!(s.run(&["split", "doc.md", "--apply"]).0, 0);
    let mut names: Vec<String> = s.tree().into_keys().collect();
    names.sort();
    let mut want: Vec<String> = [
        "doc/doc.md",
        "doc/day-1.md",
        "doc/café-bar.md",
        "doc/한글-제목.md",
        "doc/code-and-emph-link.md",
        "doc/day-1-2.md",
        "doc/section-6.md",
        "doc/doc-2.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    want.sort();
    assert_eq!(names, want);

    let long = "x".repeat(70);
    let names_doc =
        format!("# Names\n\n## CON\n\na\n\n## Nul\n\nb\n\n## {long}\n\nc\n\n## Lpt1\n\nd\n\n## Normal\n\ne\n");
    let s = Scratch::with(&[("names.md", names_doc.as_bytes())]);
    assert_eq!(s.run(&["split", "names.md", "--apply"]).0, 0);
    let mut names: Vec<String> = s.tree().into_keys().collect();
    names.sort();
    let mut want = vec![
        "names/names.md".to_string(),
        "names/section-con.md".to_string(),
        "names/section-nul.md".to_string(),
        format!("names/{}.md", "x".repeat(60)),
        "names/section-lpt1.md".to_string(),
        "names/normal.md".to_string(),
    ];
    want.sort();
    assert_eq!(names, want);
}

#[test]
fn split_keeps_the_shape_of_the_text() {
    // Setext headings become ATX, headings inside fences stay, inner headings move up.
    let s = Scratch::with(&[(
        "t.md",
        b"# T\n\n## First\n\n### Sub\n\ntext\n\n```\n## not a heading\n```\n\nSetext Two\n----------\n\nmore\n",
    )]);
    assert_eq!(s.run(&["split", "t.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("t/t.md", b"# T\n\n- [[first]]\n- [[setext-two]]\n"),
        (
            "t/first.md",
            b"# First\n\n## Sub\n\ntext\n\n```\n## not a heading\n```\n",
        ),
        ("t/setext-two.md", b"# Setext Two\n\nmore\n"),
    ]);

    // The line ending of the text is kept; the written documents end with one line ending.
    let s = Scratch::with(&[(
        "c.md",
        b"# C\r\n\r\npre\r\n\r\n## A\r\n\r\nx\r\n\r\n## B\r\n\r\ny",
    )]);
    assert_eq!(s.run(&["split", "c.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("c/c.md", b"# C\r\n\r\npre\r\n\r\n- [[a]]\r\n- [[b]]\r\n"),
        ("c/a.md", b"# A\r\n\r\nx\r\n"),
        ("c/b.md", b"# B\r\n\r\ny\r\n"),
    ]);

    // A byte order mark and front matter stay with the folder document.
    let mut m = b"\xEF\xBB\xBF".to_vec();
    m.extend_from_slice(b"---\ntags: [t]\n---\n# M\n\n## A\n\na\n");
    let mut want = b"\xEF\xBB\xBF".to_vec();
    want.extend_from_slice(b"---\ntags: [t]\n---\n# M\n\n- [[a]]\n");
    let s = Scratch::with(&[("m.md", &m)]);
    assert_eq!(s.run(&["split", "m.md", "--apply"]).0, 0);
    s.assert_tree(&[("m/m.md", &want), ("m/a.md", b"# A\n\na\n")]);
}

#[test]
fn a_folder_document_is_split_in_place() {
    let s = Scratch::with(&[("notes/notes.md", b"# Notes\n\n## One\n\n1\n\n## Two\n\n2\n")]);
    let (_, out, _) = s.run(&["split", "notes/notes.md"]);
    assert!(!out.contains("move "), "{out}");
    assert_eq!(s.run(&["split", "notes/notes.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("notes/notes.md", b"# Notes\n\n- [[one]]\n- [[two]]\n"),
        ("notes/one.md", b"# One\n\n1\n"),
        ("notes/two.md", b"# Two\n\n2\n"),
    ]);
}

#[test]
fn merge_shows_the_plan_with_the_rewritten_links() {
    let s = Scratch::with(&[
        ("a.md", b"# A\n\ntext A\n"),
        ("b.md", b"# B\n\ntext B\n\n## Part\n\ndetail\n"),
        (
            "idx.md",
            b"[x](b.md#part) [y](b.md) [[b]] [[b#Part]] [[b|bee]]\n",
        ),
    ]);
    let before = s.tree();
    let (code, out, _) = s.run(&["merge", "a.md", "b.md"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "write    a.md (adds b.md as section \"## B\")\n\
         delete   b.md (7 lines)\n\
         \n\
         rewrite  5 links in 1 document\n\
         \x20 idx.md:1\n\
         \x20   - [x](b.md#part)\n\
         \x20   + [x](a.md#part)\n\
         \x20   - [y](b.md)\n\
         \x20   + [y](a.md#b)\n\
         \x20   - [[b]]\n\
         \x20   + [[a#B]]\n\
         \x20   (b.md was merged into a.md)\n\
         \x20   - [[b#Part]]\n\
         \x20   + [[a#Part]]\n\
         \x20   (b.md was merged into a.md)\n\
         \x20   - [[b|bee]]\n\
         \x20   + [[a#B|bee]]\n\
         \x20   (b.md was merged into a.md)\n\
         \n\
         checks   nothing disappears · links intact · no name clashes\n\
         Dry run: nothing was written. Use --apply to write.\n"
    );
    assert_eq!(s.tree(), before);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        (
            "a.md",
            b"# A\n\ntext A\n\n## B\n\ntext B\n\n### Part\n\ndetail\n",
        ),
        (
            "idx.md",
            b"[x](a.md#part) [y](a.md#b) [[a#B]] [[a#Part]] [[a#B|bee]]\n",
        ),
    ]);
    assert_eq!(s.run(&["check"]).0, 0);
}

#[test]
fn merge_keeps_every_link_meaning_what_it_meant() {
    let png: Vec<u8> = (0..=255u8).collect();
    let s = Scratch::with(&[
        ("x/a.md", b"# A\n\n[b](../y/b.md)\n"),
        (
            "y/b.md",
            b"# B\n\nSee [pic](pic.png) and [[c]] and [self](b.md).\n",
        ),
        ("y/pic.png", &png),
        ("y/c.md", b"c\n"),
        ("x/c.md", b"xc\n"),
        ("z/c.md", b"zz\n"),
        ("idx.md", b"[b](y/b.md) [[b]]\n"),
    ]);
    assert_eq!(s.run(&["merge", "x/a.md", "y/b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        (
            "x/a.md",
            b"# A\n\n[b](a.md#b)\n\n## B\n\nSee [pic](../y/pic.png) and [[y/c]] and [self](a.md#b).\n",
        ),
        ("idx.md", b"[b](x/a.md#b) [[a#B]]\n"),
        ("y/pic.png", &png),
        ("y/c.md", b"c\n"),
        ("x/c.md", b"xc\n"),
        ("z/c.md", b"zz\n"),
    ]);
}

#[test]
fn merge_renumbers_anchors_the_way_the_merged_document_would() {
    let s = Scratch::with(&[
        ("a.md", b"# A\n\n## Part\n\nA part\n"),
        ("b.md", b"# B\n\n## Part\n\nB part\n\nSee [up](#part).\n"),
        ("idx.md", b"[1](a.md#part) [2](b.md#part)\n"),
    ]);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        (
            "a.md",
            b"# A\n\n## Part\n\nA part\n\n## B\n\n### Part\n\nB part\n\nSee [up](#part-1).\n",
        ),
        ("idx.md", b"[1](a.md#part) [2](a.md#part-1)\n"),
    ]);
}

#[test]
fn merge_names_a_repeated_heading_by_its_anchor_in_a_wiki_link() {
    let s = Scratch::with(&[
        ("a.md", b"# A\n\n## More\n\nx\n"),
        ("b.md", b"# B\n\n## More\n\ny [[#More]]\n"),
        ("idx.md", b"[[b#More]] [[a#More]]\n"),
    ]);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        (
            "a.md",
            b"# A\n\n## More\n\nx\n\n## B\n\n### More\n\ny [[#more-1]]\n",
        ),
        ("idx.md", b"[[a#more-1]] [[a#More]]\n"),
    ]);
}

#[test]
fn merge_several_documents_in_the_order_given() {
    let s = Scratch::with(&[
        ("a.md", b"# A\n"),
        ("b.md", b"# B\n\nb\n"),
        ("c.md", b"# C\n\nc\n"),
    ]);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "c.md", "--apply"]).0, 0);
    s.assert_tree(&[("a.md", b"# A\n\n## B\n\nb\n\n## C\n\nc\n")]);
}

#[test]
fn merge_gives_a_document_without_a_title_one_from_its_name() {
    let s = Scratch::with(&[("a.md", b"# A\n"), ("notes.md", b"just text\n")]);
    assert_eq!(s.run(&["merge", "a.md", "notes.md", "--apply"]).0, 0);
    s.assert_tree(&[("a.md", b"# A\n\n## notes\n\njust text\n")]);
}

#[test]
fn merge_refuses_what_would_lose_or_change_something() {
    let cases: &[(Files, &[&str], i32, &str)] = &[
        (
            &[("a.md", b"# A\n"), ("b.md", b"---\ntags: [t]\n---\n# B\n")],
            &["a.md", "b.md"],
            1,
            "refused [I1] b.md",
        ),
        (
            &[("a.md", b"# A\n"), ("b.md", b"# B\n\n###### Deep\n")],
            &["a.md", "b.md"],
            1,
            "refused [I1] b.md:3:",
        ),
        (
            &[
                ("a.md", b"# A\n\n[r]: x.md\n"),
                ("b.md", b"# B\n\nSee [t][r].\n\n[r]: y.md\n"),
            ],
            &["a.md", "b.md"],
            1,
            "refused [I2] b.md:5:",
        ),
        (
            &[
                ("a.md", b"# A\n\nplain [r] text\n"),
                ("b.md", b"# B\n\n[r]: y.md\n"),
            ],
            &["a.md", "b.md"],
            1,
            "refused [I2] a.md",
        ),
        (
            &[
                ("a.md", b"# A\n\nT[^1].\n\n[^1]: a\n"),
                ("b.md", b"# B\n\nU[^1].\n\n[^1]: b\n"),
            ],
            &["a.md", "b.md"],
            1,
            "refused [I2] b.md",
        ),
    ];
    for (files, args, code, message) in cases {
        let s = Scratch::with(files);
        let before = s.tree();
        let mut argv = vec!["merge"];
        argv.extend_from_slice(args);
        argv.push("--apply");
        let (got, _, err) = s.run(&argv);
        assert_eq!(got, *code, "{args:?}: {err}");
        assert!(err.contains(message), "{args:?}: {err}");
        assert_eq!(s.tree(), before, "{args:?}");
    }
}

#[test]
fn merge_is_a_usage_error_for_things_that_cannot_be_merged() {
    let cases: &[&[&str]] = &[
        &["a.md", "a.md"],
        &["a.md", "zz.md"],
        &["a.md", "p.png"],
        &["a.md", "f"],
        &["a.md", "b.md", "b.md"],
    ];
    for args in cases {
        let s = Scratch::with(&[
            ("a.md", b"# A\n"),
            ("b.md", b"# B\n"),
            ("p.png", b"x"),
            ("f/x.md", b"x\n"),
        ]);
        let before = s.tree();
        let mut argv = vec!["merge"];
        argv.extend_from_slice(args);
        argv.push("--apply");
        assert_eq!(s.run(&argv).0, 2, "{args:?}");
        assert_eq!(s.tree(), before);
    }
}

#[test]
fn merge_obeys_the_budget() {
    let s = Scratch::with(&[
        ("a.md", b"# A\n"),
        ("b.md", b"# B\n"),
        ("idx.md", b"[b](b.md)\n"),
    ]);
    let before = s.tree();
    let (code, _, err) = s.run(&["merge", "a.md", "b.md", "--max-touched", "1", "--apply"]);
    assert_eq!(code, 1);
    assert!(err.contains("refused [I4]"), "{err}");
    assert_eq!(s.tree(), before);
}

#[test]
fn a_saved_split_plan_applies_to_the_same_tree_and_only_once() {
    let a = Scratch::with(&[("notes/trip.md", TRIP), ("i.md", b"[[trip#Day 2]]\n")]);
    let b = Scratch::with(&[("notes/trip.md", TRIP), ("i.md", b"[[trip#Day 2]]\n")]);
    let plan = std::env::temp_dir().join(format!("textree-split-plan-{}.json", std::process::id()));
    let plan_s = plan.to_string_lossy().to_string();
    assert_eq!(a.run(&["split", "notes/trip.md", "--plan", &plan_s]).0, 0);
    assert_eq!(b.run(&["split", "notes/trip.md", "--apply"]).0, 0);
    assert_eq!(a.run(&["apply", &plan_s]).0, 0);
    assert_eq!(a.tree(), b.tree());
    assert_eq!(a.run(&["apply", &plan_s]).0, 1);
    let _ = fs::remove_file(plan);
}

#[test]
fn merge_points_a_link_to_a_merged_document_past_an_earlier_heading_that_reads_the_same() {
    let s = Scratch::with(&[
        ("a.md", b"# Same\n\nx\n"),
        ("b.md", b"# Same\n\nbb\n"),
        ("idx.md", b"[[b]] [[b#Same]] [[a#Same]] [b](b.md)\n"),
    ]);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("a.md", b"# Same\n\nx\n\n## Same\n\nbb\n"),
        (
            "idx.md",
            b"[[a#same-1]] [[a#same-1]] [[a#Same]] [b](a.md#same-1)\n",
        ),
    ]);
    let s = Scratch::with(&[
        ("a.md", b"# A\n\n## Part\n\nx\n"),
        ("b.md", b"# part\n\nbb\n"),
        ("idx.md", b"[[b]]\n"),
    ]);
    assert_eq!(s.run(&["merge", "a.md", "b.md", "--apply"]).0, 0);
    s.assert_tree(&[
        ("a.md", b"# A\n\n## Part\n\nx\n\n## part\n\nbb\n"),
        ("idx.md", b"[[a#part-1]]\n"),
    ]);
}
