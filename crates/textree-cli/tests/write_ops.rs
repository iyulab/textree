//! `textree add` and `textree replace`: new text goes in through the same plan and checks as
//! every other change.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("textree-write-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, path: &str, body: &str) -> &Self {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
        self
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `textree <cmd> <folder> <args>... -` with `text` on standard input.
fn with_text(s: &Scratch, cmd: &str, args: &[&str], text: &str) -> (i32, String, String) {
    let mut all = vec![cmd, s.0.to_str().unwrap()];
    all.extend_from_slice(args);
    let mut child = Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(&all)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn check(s: &Scratch) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(["check", s.0.to_str().unwrap()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn add_writes_exactly_the_text_and_its_links_resolve() {
    let s = Scratch::new();
    s.file("notes/trip.md", "# Trip\n")
        .file("index.md", "# Index\n\n[[trip]]\n");
    let text = "# New\n\nSee [[trip]].\n";
    let (code, out, err) = with_text(&s, "add", &["notes/new.md", "-"], text);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("create   notes/new.md (new document, 3 lines)\n"),
        "{out}"
    );
    assert!(!s.0.join("notes/new.md").exists(), "a plan writes nothing");
    let (code, out, err) = with_text(&s, "add", &["notes/new.md", "-", "--apply"], text);
    assert_eq!(code, 0, "{err}");
    assert!(out.ends_with("Applied: 1 created.\n"), "{out}");
    assert_eq!(s.read("notes/new.md"), text);
    assert_eq!(check(&s), "no broken links\n");
}

#[test]
fn add_ends_the_text_with_one_line_break() {
    let s = Scratch::new();
    s.file("a.md", "# A\n");
    let (code, _, err) = with_text(&s, "add", &["b.md", "-", "--apply"], "# B");
    assert_eq!(code, 0, "{err}");
    assert_eq!(s.read("b.md"), "# B\n");
}

#[test]
fn add_refuses_a_path_that_is_taken() {
    let s = Scratch::new();
    s.file("a.md", "# A\n");
    for path in ["a.md", "A.md"] {
        let (code, _, err) = with_text(&s, "add", &[path, "-"], "# X\n");
        assert_eq!(code, 1, "{path}: {err}");
        assert!(err.starts_with(&format!("refused [I5] {path}")), "{err}");
    }
    assert_eq!(s.read("a.md"), "# A\n");
}

#[test]
fn add_allows_a_broken_link_in_the_new_text_and_says_so() {
    let s = Scratch::new();
    s.file("a.md", "# A\n");
    let (code, out, err) = with_text(&s, "add", &["b.md", "-"], "# B\n\n[[nowhere]]\n");
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("broken   b.md:3: [[nowhere]] (in the new text; allowed)\n"),
        "{out}"
    );
}

#[test]
fn add_keeps_a_name_link_on_the_document_it_meant() {
    let s = Scratch::new();
    s.file("archive/trip.md", "# Trip\n")
        .file("index.md", "# Index\n\n[[trip]]\n");
    let (code, out, err) = with_text(&s, "add", &["trip.md", "-", "--apply"], "# Trip now\n");
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("    - [[trip]]\n    + [[archive/trip]]\n"),
        "{out}"
    );
    assert_eq!(s.read("index.md"), "# Index\n\n[[archive/trip]]\n");
}

#[test]
fn add_lets_a_link_that_waited_for_the_document_find_it() {
    let s = Scratch::new();
    s.file("index.md", "# Index\n\n[[later]]\n");
    let (code, _, err) = with_text(&s, "add", &["later.md", "-", "--apply"], "# Later\n");
    assert_eq!(code, 0, "{err}");
    assert_eq!(check(&s), "no broken links\n");
}

#[test]
fn replace_writes_the_new_text_and_links_to_it_keep_working() {
    let s = Scratch::new();
    s.file("doc.md", "# Doc\n\n## Kept\n\nOld text [[a]].\n")
        .file("a.md", "# A\n\n[[doc]] [[doc#Kept]]\n");
    let text = "# Doc\n\n## Kept\n\nNew text, [[a]] again.\n";
    let (code, out, err) = with_text(&s, "replace", &["doc.md", "-", "--apply"], text);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("write    doc.md (new text, 5 lines replacing 5 lines)\n"),
        "{out}"
    );
    assert!(out.ends_with("Applied: 1 document written.\n"), "{out}");
    assert_eq!(s.read("doc.md"), text);
    assert_eq!(check(&s), "no broken links\n");
}

#[test]
fn replace_refuses_to_drop_a_heading_another_document_names() {
    let s = Scratch::new();
    s.file("doc.md", "# Doc\n\n## Old\n\nText.\n")
        .file("a.md", "# A\n\n[[doc#Old]]\n");
    let (code, _, err) = with_text(&s, "replace", &["doc.md", "-"], "# Doc\n\n## New\n");
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "refused [I2] a.md:3: [[doc#Old]] names a heading the new text does not have\n"
    );
    assert_eq!(s.read("doc.md"), "# Doc\n\n## Old\n\nText.\n");
}

#[test]
fn replace_may_leave_a_document_unlinked_and_says_so() {
    let s = Scratch::new();
    s.file("doc.md", "# Doc\n\n[[b]]\n").file("b.md", "# B\n");
    let (code, out, err) = with_text(&s, "replace", &["doc.md", "-"], "# Doc\n\nNo links.\n");
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("orphan   b.md (nothing links to it any more; allowed)\n"),
        "{out}"
    );
}

#[test]
fn replace_needs_an_existing_document() {
    let s = Scratch::new();
    s.file("a.md", "# A\n");
    let (code, _, err) = with_text(&s, "replace", &["nope.md", "-"], "# X\n");
    assert_eq!(code, 2);
    assert_eq!(err, "error: no such document: nope.md\n");
}
