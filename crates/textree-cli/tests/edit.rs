//! `textree move`, `delete` and `apply`, run as a user would run them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("textree-edit-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, path: &str, body: impl AsRef<[u8]>) -> &Self {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
        self
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn read(&self, path: &str) -> String {
        String::from_utf8(fs::read(self.0.join(path)).unwrap()).unwrap()
    }

    /// Exact-case file names with their bytes, and the folders.
    fn state(&self) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
        fn walk(root: &Path, dir: &Path, f: &mut BTreeMap<String, Vec<u8>>, d: &mut Vec<String>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
                if p.is_dir() {
                    d.push(rel);
                    walk(root, &p, f, d);
                } else {
                    f.insert(rel, fs::read(&p).unwrap());
                }
            }
        }
        let (mut f, mut d) = (BTreeMap::new(), Vec::new());
        walk(&self.0, &self.0, &mut f, &mut d);
        d.sort();
        (f, d)
    }

    fn names(&self) -> Vec<String> {
        self.state().0.keys().cloned().collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fn open_up(dir: &Path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
            }
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        open_up(&e.path());
                    }
                }
            }
        }
        open_up(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn textree(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(args)
        .output()
        .unwrap()
}

fn run(s: &Scratch, args: &[&str]) -> (i32, String, String) {
    let mut all: Vec<&str> = vec![args[0], s.path().to_str().unwrap()];
    all.extend_from_slice(&args[1..]);
    let out = textree(&all);
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const PNG: [u8; 7] = [0x89, 0x50, 0x4e, 0x47, 1, 2, 3];

fn soil(s: &Scratch) {
    s.file("index.md", "See [soil](garden/soil.md) and [[soil]].\n")
        .file(
            "garden/soil.md",
            "# Soil\n\nBack to [home](../index.md) and ![beds](beds.png) and [[index]].\n",
        )
        .file("garden/beds.png", PNG);
}

// --- S2-1 ---------------------------------------------------------------------------------

#[test]
fn the_plan_is_shown_first_and_nothing_is_written() {
    let s = Scratch::new();
    soil(&s);
    let before = s.state();
    let (code, out, err) = run(&s, &["move", "garden/soil.md", "archive/soil.md"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "move     garden/soil.md -> archive/soil.md\n\
         \n\
         rewrite  2 links in 2 documents\n\
         \x20 archive/soil.md:3\n\
         \x20   - ![beds](beds.png)\n\
         \x20   + ![beds](../garden/beds.png)\n\
         \x20 index.md:1\n\
         \x20   - [soil](garden/soil.md)\n\
         \x20   + [soil](archive/soil.md)\n\
         \n\
         checks   content kept · links intact · no empty folders · no name clashes\n\
         Dry run: nothing was written. Use --apply to write.\n"
    );
    assert_eq!(s.state(), before);
}

#[test]
fn apply_writes_the_planned_tree() {
    let s = Scratch::new();
    soil(&s);
    let (code, out, err) = run(
        &s,
        &["move", "garden/soil.md", "archive/soil.md", "--apply"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out
        .trim_end()
        .lines()
        .last()
        .unwrap()
        .starts_with("Applied"));
    assert_eq!(
        s.read("index.md"),
        "See [soil](archive/soil.md) and [[soil]].\n"
    );
    assert_eq!(
        s.read("archive/soil.md"),
        "# Soil\n\nBack to [home](../index.md) and ![beds](../garden/beds.png) and [[index]].\n"
    );
    assert_eq!(
        s.names(),
        vec!["archive/soil.md", "garden/beds.png", "index.md"]
    );
    let (code, out, _) = run(&s, &["check"]);
    assert_eq!((code, out.as_str()), (0, "no broken links\n"));
}

// --- S2-2 / S2-4 ----------------------------------------------------------------------------

#[test]
fn renaming_a_folder_renames_its_folder_document_and_fixes_the_name_link() {
    let s = Scratch::new();
    s.file(
        "projects/projects.md",
        "Garden: [[garden]] and [g](garden/garden.md) and [w](garden/watering.md#schedule).\n",
    )
    .file(
        "projects/garden/garden.md",
        "# Garden\n\nSee [watering](watering.md) and [up](../projects.md).\n",
    )
    .file(
        "projects/garden/watering.md",
        "# Watering\n\n## Schedule\n\nDaily.\n",
    );
    let (code, _, err) = run(&s, &["move", "projects/garden", "projects/yard", "--apply"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        s.names(),
        vec![
            "projects/projects.md",
            "projects/yard/watering.md",
            "projects/yard/yard.md"
        ]
    );
    assert_eq!(
        s.read("projects/projects.md"),
        "Garden: [[yard]] and [g](yard/yard.md) and [w](yard/watering.md#schedule).\n"
    );
    assert_eq!(
        s.read("projects/yard/yard.md"),
        "# Garden\n\nSee [watering](watering.md) and [up](../projects.md).\n"
    );
    assert_eq!(run(&s, &["check"]).0, 0);
}

#[test]
fn a_case_only_rename_leaves_the_new_spelling_on_disk() {
    let s = Scratch::new();
    s.file("Note.md", "# N\n")
        .file("ref.md", "[a](Note.md) [[Note]]\n");
    let (code, _, err) = run(&s, &["move", "Note.md", "note.md", "--apply"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(s.names(), vec!["note.md", "ref.md"]);
    assert_eq!(s.read("ref.md"), "[a](note.md) [[Note]]\n");
    assert_eq!(run(&s, &["check"]).0, 0);
}

// --- S2-5 ---------------------------------------------------------------------------------

fn small(s: &Scratch) {
    s.file("a.md", "A [b](b.md)\n")
        .file("b.md", "B\n")
        .file("d/one.md", "1\n")
        .file("d/two.md", "2\n");
}

#[test]
fn refused_changes_write_nothing_and_say_which_invariant() {
    let s = Scratch::new();
    small(&s);
    let before = s.state();
    for (args, needle) in [
        (vec!["move", "a.md", "b.md", "--apply"], "refused [I5] b.md"),
        (vec!["move", "a.md", "B.md", "--apply"], "refused [I5] B.md"),
        (vec!["move", "d", "d/sub", "--apply"], "into itself"),
    ] {
        let (code, out, err) = run(&s, &args);
        assert_eq!(code, 1, "{args:?}: {out} {err}");
        assert!(err.contains(needle), "{args:?}: {err}");
        assert_eq!(s.state(), before, "{args:?}");
    }
}

#[test]
fn bad_requests_are_usage_errors() {
    let s = Scratch::new();
    small(&s);
    let before = s.state();
    for args in [
        vec!["move", "nope.md", "z.md"],
        vec!["move", "a.md", "../outside.md"],
        vec!["move", "a.md", "/tmp/x.md"],
        vec!["move", "a.md", "d/./x.md"],
        vec!["delete", "nope.md"],
    ] {
        let (code, _, err) = run(&s, &args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(!err.is_empty());
    }
    assert_eq!(run(&s, &["move", "a.md", "a.md", "--apply"]).0, 0);
    assert_eq!(s.state(), before);
}

#[test]
fn the_budget_can_refuse_a_wide_change() {
    let s = Scratch::new();
    s.file("t.md", "t\n");
    for i in 1..=8 {
        s.file(&format!("l{i}.md"), "[t](t.md)\n");
    }
    let before = s.state();
    let (code, _, err) = run(
        &s,
        &["move", "t.md", "sub/t.md", "--max-touched", "3", "--apply"],
    );
    assert_eq!(code, 1);
    assert!(err.contains("refused [I4]"), "{err}");
    assert_eq!(s.state(), before);
    assert_eq!(
        run(
            &s,
            &["move", "t.md", "sub/t.md", "--max-touched", "9", "--apply"]
        )
        .0,
        0
    );
}

// --- S2-6 ---------------------------------------------------------------------------------

fn deletable(s: &Scratch) {
    s.file("a.md", "See [b](b.md) and [[c]].\n")
        .file("b.md", "x\n")
        .file("c.md", "y\n")
        .file("lonely.md", "z\n")
        .file("sub/s1.md", "s\n");
}

#[test]
fn deleting_shows_what_goes_and_refuses_what_would_break_links() {
    let s = Scratch::new();
    deletable(&s);
    let before = s.state();
    let (code, out, _) = run(&s, &["delete", "lonely.md"]);
    assert_eq!(code, 0);
    assert!(out.contains("delete   lonely.md"), "{out}");
    assert!(out.contains("Dry run"));
    assert_eq!(s.state(), before);
    assert_eq!(run(&s, &["delete", "lonely.md", "--apply"]).0, 0);
    assert!(!s.names().contains(&"lonely.md".to_string()));
    assert_eq!(run(&s, &["check"]).0, 0);

    let kept = s.state();
    let (code, _, err) = run(&s, &["delete", "b.md", "--apply"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("refused [I2] a.md:1: [b](b.md) would break"),
        "{err}"
    );
    assert_eq!(s.state(), kept);

    let (code, out, _) = run(&s, &["delete", "b.md", "--break-links", "--apply"]);
    assert_eq!(code, 0);
    assert!(out.contains("delete   b.md"));
    assert_eq!(s.read("a.md"), "See [b](b.md) and [[c]].\n");
    let (code, out, _) = run(&s, &["check"]);
    assert_eq!(code, 1);
    assert!(out.contains("a.md:1: broken link [b](b.md)"), "{out}");
}

#[test]
fn deleting_a_folder_removes_its_documents_and_the_folder() {
    let s = Scratch::new();
    deletable(&s);
    let (code, out, err) = run(&s, &["delete", "sub", "--apply"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("delete   sub/s1.md"), "{out}");
    let (files, dirs) = s.state();
    assert!(!files.contains_key("sub/s1.md"));
    assert!(!dirs.contains(&"sub".to_string()), "{dirs:?}");
}

// --- S2-7 ---------------------------------------------------------------------------------

fn bystander(s: &Scratch) {
    s.file("x/d/note.md", "# Intro\n")
        .file("y/note.md", "y\n")
        .file("p/other.md", "Bystander: [[note]]\n")
        .file(
            "x/link.md",
            "Mover: [[note]] [[note|the note]] [[note#Intro]] ![[note]]\n",
        );
}

#[test]
fn a_saved_plan_applies_once_and_only_to_the_folder_it_was_made_for() {
    let s = Scratch::new();
    bystander(&s);
    let plan = s.path().join("../plan-a.json");
    let plan = plan.to_str().unwrap().to_string();
    let before = s.state();
    let (code, _, err) = run(&s, &["move", "x/d/note.md", "x/note.md", "--plan", &plan]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(s.state(), before, "--plan must not write to the folder");
    let (code, out, err) = run(&s, &["apply", &plan]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(s.read("p/other.md"), "Bystander: [[y/note]]\n");
    let (code, _, err) = run(&s, &["apply", &plan]);
    assert_eq!(code, 1);
    assert!(err.contains("stale"), "{err}");
    let _ = fs::remove_file(&plan);
}

#[test]
fn a_plan_is_stale_after_an_edit_a_taken_destination_or_a_new_document() {
    for (label, change) in [
        ("edit", ("y/note.md", "changed\n")),
        ("destination", ("x/note.md", "someone else's\n")),
        ("new document", ("q/note.md", "a new note\n")),
    ] {
        let s = Scratch::new();
        bystander(&s);
        let plan = s.path().join("../plan-b.json");
        let plan = plan.to_str().unwrap().to_string();
        assert_eq!(
            run(&s, &["move", "x/d/note.md", "x/note.md", "--plan", &plan]).0,
            0
        );
        s.file(change.0, change.1);
        let before = s.state();
        let (code, _, err) = run(&s, &["apply", &plan]);
        assert_eq!(code, 1, "{label}: {err}");
        assert!(err.contains("stale"), "{label}: {err}");
        assert_eq!(s.state(), before, "{label}: nothing may be written");
        let _ = fs::remove_file(&plan);
    }
}

#[test]
fn a_tampered_or_missing_plan_is_a_usage_error() {
    let s = Scratch::new();
    bystander(&s);
    let before = s.state();
    let evil = std::env::temp_dir().join(format!("textree-evil-{}.json", std::process::id()));
    for body in [
        r#"{"version":1,"nodes_fingerprint":"x","reads":[],"moves":[{"from":"x/link.md","to":"../escaped.md"}],"writes":[],"deletes":[],"remove_folders":[],"rewrites":[]}"#,
        r#"{"version":1,"nodes_fingerprint":"x","reads":[],"moves":[],"writes":[{"path":"/etc/x","content":"x"}],"deletes":[],"remove_folders":[],"rewrites":[]}"#,
        r#"{"version":1,"nodes_fingerprint":"x","reads":[],"moves":[],"writes":[],"deletes":[{"path":"a/../../b","kind":"document"}],"remove_folders":[],"rewrites":[]}"#,
        "this is not json",
    ] {
        fs::write(&evil, body).unwrap();
        let (code, _, err) = run(&s, &["apply", evil.to_str().unwrap()]);
        assert_eq!(code, 2, "{body}: {err}");
        assert_eq!(s.state(), before);
    }
    let _ = fs::remove_file(&evil);
    assert_eq!(run(&s, &["apply", "/nonexistent/plan.json"]).0, 2);
}

// --- S2-8 ---------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_failure_while_writing_leaves_the_folder_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    bystander(&s);
    s.file("p/second.md", "[[note]]\n")
        .file("r/third.md", "[[note]]\n");
    let before = s.state();
    fs::set_permissions(s.path().join("r"), fs::Permissions::from_mode(0o555)).unwrap();
    let (code, _, err) = run(&s, &["move", "x/d/note.md", "x/note.md", "--apply"]);
    fs::set_permissions(s.path().join("r"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 1);
    assert!(err.contains("r/third.md"), "{err}");
    assert_eq!(s.state(), before);
}

// --- S2-9 ---------------------------------------------------------------------------------

#[test]
fn rewriting_touches_link_bytes_only() {
    let s = Scratch::new();
    let hub = "# Hub\n[a](old/note.md#part) ![img](old/pic.png) [b](<old/my note.md>) [c](old/my%20note.md \"Title\")\n[d][ref] [e][] [[old/note|alias]] `[x](old/note.md)`\n\n```\n[x](old/note.md)\n```\n\n[ref]: old/note.md \"Ref title\"\n[e]: old/note.md\n";
    s.file("hub.md", hub)
        .file("crlf.md", b"a\r\n[l](old/note.md)\r\n")
        .file(
            "bom.md",
            [&[0xEF, 0xBB, 0xBF][..], b"[l](old/note.md)\n"].concat(),
        )
        .file("old/note.md", "x\n")
        .file("old/my note.md", "y\n")
        .file("old/pic.png", PNG);
    let (code, _, err) = run(&s, &["move", "old", "new", "--apply"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        s.read("hub.md"),
        "# Hub\n[a](new/note.md#part) ![img](new/pic.png) [b](<new/my note.md>) [c](new/my%20note.md \"Title\")\n[d][ref] [e][] [[new/note|alias]] `[x](old/note.md)`\n\n```\n[x](old/note.md)\n```\n\n[ref]: new/note.md \"Ref title\"\n[e]: new/note.md\n"
    );
    assert_eq!(
        fs::read(s.path().join("crlf.md")).unwrap(),
        b"a\r\n[l](new/note.md)\r\n"
    );
    assert_eq!(
        fs::read(s.path().join("bom.md")).unwrap(),
        [&[0xEF, 0xBB, 0xBF][..], b"[l](new/note.md)\n"].concat()
    );
    assert_eq!(fs::read(s.path().join("new/pic.png")).unwrap(), PNG);
    assert!(!s.path().join("old").exists());
    assert_eq!(run(&s, &["check"]).0, 0);
}

#[test]
fn links_that_leave_the_root_follow_the_document_and_broken_ones_stay() {
    let s = Scratch::new();
    s.file("doc.md", "[o](../outside/z.md) [m](missing.md)\n");
    assert_eq!(run(&s, &["move", "doc.md", "sub/doc.md", "--apply"]).0, 0);
    assert_eq!(
        s.read("sub/doc.md"),
        "[o](../../outside/z.md) [m](missing.md)\n"
    );
}

// --- S2-12 / S2-14 ----------------------------------------------------------------------------

#[test]
fn tags_start_with_a_letter_and_collapsed_references_cover_their_brackets() {
    let s = Scratch::new();
    s.file(
        "t.md",
        "Issue #1577에 and #2는 and #123 and #_x and #태그 and #a1 and #2024-goals.\n",
    )
    .file(
        "r.md",
        "See [x][] and [y] and [z][w].\n\n[x]: n.md\n[y]: n.md\n[w]: n.md\n",
    )
    .file("n.md", "n\n");
    let (code, out, _) = run(&s, &["graph", "--json"]);
    assert_eq!(code, 0);
    let g: serde_json::Value = serde_json::from_str(&out).unwrap();
    let tags: Vec<&str> = g["tags"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(tags, vec!["_x", "a1", "태그"]);
    let src = fs::read(s.path().join("r.md")).unwrap();
    let cut: Vec<String> = g["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["from"] == "r.md")
        .map(|l| {
            let (a, b) = (
                l["range"]["start"].as_u64().unwrap() as usize,
                l["range"]["end"].as_u64().unwrap() as usize,
            );
            String::from_utf8(src[a..b].to_vec()).unwrap()
        })
        .collect();
    assert_eq!(cut, vec!["[x][]", "[y]", "[z][w]"]);
}

#[test]
fn plans_are_the_same_whatever_the_creation_order_or_working_directory() {
    let make = |order: &[&str]| {
        let s = Scratch::new();
        let all = [
            ("x/d/note.md", "# Intro\n"),
            ("y/note.md", "y\n"),
            ("p/other.md", "Bystander: [[note]]\n"),
            (
                "x/link.md",
                "Mover: [[note]] [[note|the note]] [[note#Intro]] ![[note]]\n",
            ),
        ];
        for name in order {
            let (p, c) = all.iter().find(|(p, _)| p == name).unwrap();
            s.file(p, c);
        }
        s
    };
    let a = make(&["x/d/note.md", "y/note.md", "p/other.md", "x/link.md"]);
    let b = make(&["x/link.md", "p/other.md", "y/note.md", "x/d/note.md"]);
    let plan_of = |s: &Scratch| {
        let f = std::env::temp_dir().join(format!(
            "textree-det-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let (code, _, err) = run(
            s,
            &[
                "move",
                "x/d/note.md",
                "x/note.md",
                "--plan",
                f.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 0, "{err}");
        let text = fs::read_to_string(&f).unwrap();
        let _ = fs::remove_file(&f);
        text
    };
    let (pa, pb) = (plan_of(&a), plan_of(&b));
    assert_eq!(pa, pb, "the plan depends on creation order");
    assert!(
        !pa.contains(a.path().to_str().unwrap()),
        "the plan names the folder's absolute path"
    );
    // And the trees after applying are the same.
    assert_eq!(
        run(&a, &["move", "x/d/note.md", "x/note.md", "--apply"]).0,
        0
    );
    assert_eq!(
        run(&b, &["move", "x/d/note.md", "x/note.md", "--apply"]).0,
        0
    );
    assert_eq!(a.state(), b.state());
}

// --- found by the evaluation ----------------------------------------------------------------

#[test]
fn a_hidden_destination_is_refused_before_anything_is_written() {
    let s = Scratch::new();
    s.file("idx.md", "[a](n.md) [[n]]\n").file("n.md", "n\n");
    let before = s.state();
    for to in [".n.md", ".d/n.md"] {
        let (code, out, err) = run(&s, &["move", "n.md", to, "--apply"]);
        assert_eq!(code, 1, "{to}: {out} {err}");
        assert!(
            err.contains("refused [I5]") && err.contains("hidden"),
            "{err}"
        );
        assert!(
            !out.contains("links intact"),
            "the plan must not claim a good plan: {out}"
        );
        assert_eq!(s.state(), before, "{to}");
    }
}

#[cfg(unix)]
#[test]
fn a_destination_behind_a_symbolic_link_is_refused() {
    let outside = Scratch::new();
    let s = Scratch::new();
    s.file("a.md", "a\n").file("b.md", "[a](a.md)\n");
    std::os::unix::fs::symlink(outside.path(), s.path().join("lnk")).unwrap();
    let before = s.state();
    let (code, out, err) = run(&s, &["move", "a.md", "lnk/a.md", "--apply"]);
    assert_eq!(code, 1, "{out} {err}");
    assert!(err.contains("symbolic link"), "{err}");
    assert_eq!(s.state(), before);
    assert!(
        fs::read_dir(outside.path()).unwrap().next().is_none(),
        "something left the root"
    );

    // A forged plan that goes through the link is not applied either.
    let plan = std::env::temp_dir().join(format!("textree-forged-{}.json", std::process::id()));
    fs::write(
        &plan,
        r#"{"version":1,"nodes_fingerprint":"x","reads":[],"moves":[],"writes":[{"path":"lnk/victim.md","content":"PWNED"}],"deletes":[],"remove_folders":[],"rewrites":[]}"#,
    )
    .unwrap();
    let (code, _, err) = run(&s, &["apply", plan.to_str().unwrap()]);
    let _ = fs::remove_file(&plan);
    assert_eq!(code, 2, "{err}");
    assert!(!outside.path().join("victim.md").exists());
}

#[test]
fn several_rewrites_on_one_line_share_one_heading() {
    let s = Scratch::new();
    s.file("hub.md", "[a](t.md) [b](t.md#x) [[t]]\n")
        .file("t.md", "t\n")
        .file("z/z.md", "z\n");
    let (code, out, err) = run(&s, &["move", "t.md", "z/t.md"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("  hub.md:1").count(), 1, "{out}");
    assert_eq!(out.matches("    - ").count(), 2, "{out}");
}

#[test]
fn any_file_name_can_be_given_to_a_document_that_only_path_links_reach() {
    for name in ["my (draft).md", "a#b.md", "x[1].md", "q?.md", "50% done.md"] {
        let s = Scratch::new();
        s.file("a.md", "[t](t.md) [t again](t.md#top)\n")
            .file("t.md", "t\n");
        let (code, out, err) = run(&s, &["move", "t.md", name, "--apply"]);
        assert_eq!(code, 0, "{name}: {out} {err}");
        assert_eq!(run(&s, &["check"]).0, 0, "{name}");
    }
}

#[test]
fn a_name_a_wiki_link_cannot_hold_is_refused_and_one_it_can_hold_is_not() {
    // `[[my (draft)]]` is a fine wiki link.
    let s = Scratch::new();
    s.file("a.md", "[t](t.md) [[t]]\n").file("t.md", "t\n");
    let (code, out, err) = run(&s, &["move", "t.md", "my (draft).md", "--apply"]);
    assert_eq!(code, 0, "{out} {err}");
    assert_eq!(run(&s, &["check"]).0, 0);
    // `[[a#b]]` would mean heading `b` of `a`, and `[[x[1]]]` is not a link: refuse, change nothing.
    for name in ["a#b.md", "x[1].md"] {
        let s = Scratch::new();
        s.file("a.md", "[t](t.md) [[t]]\n").file("t.md", "t\n");
        let before = s.state();
        let (code, _, err) = run(&s, &["move", "t.md", name, "--apply"]);
        assert_eq!(code, 1, "{name}: {err}");
        assert!(err.contains("refused [I2]"), "{name}: {err}");
        assert_eq!(s.state(), before, "{name}");
    }
}

#[test]
fn graph_json_shows_where_reference_definitions_are() {
    let s = Scratch::new();
    s.file("r.md", "See [x][].\n\n[x]: n.md\n")
        .file("n.md", "n\n");
    let (_, out, _) = run(&s, &["graph", "--json"]);
    let g: serde_json::Value = serde_json::from_str(&out).unwrap();
    let r = g["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "r.md")
        .unwrap();
    assert_eq!(r["definitions"][0]["label"], "x");
    assert_eq!(r["definitions"][0]["line"], 3);
}
