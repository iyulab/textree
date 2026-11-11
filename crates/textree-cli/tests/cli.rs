//! The command line tool, run as a user would run it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch folder that is removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("textree-cli-test-{}-{n}", std::process::id()));
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn textree(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(args)
        .arg(dir)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn corpus_tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/corpus/tree")
}

#[test]
fn check_lists_broken_links_with_file_and_line_and_exits_1() {
    let s = Scratch::new();
    s.file(
        "a.md",
        "# A\n\nsee [[nope]] here\n\nand [x](missing.md) and [ok](b.md)\n\n[[Sub/Missing]]\n",
    )
    .file("b.md", "# B\n");
    let out = textree(&["check"], s.path());
    assert_eq!(
        stdout(&out),
        "a.md:3: broken link [[nope]]\na.md:5: broken link [x](missing.md)\na.md:7: broken link [[Sub/Missing]]\n"
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn check_on_a_clean_folder_says_so_and_exits_0() {
    let s = Scratch::new();
    s.file("a.md", "see [b](b.md)\n").file("b.md", "# B\n");
    let out = textree(&["check"], s.path());
    assert_eq!(stdout(&out), "no broken links\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_missing_folder_is_a_usage_error() {
    let out = textree(&["check"], Path::new("/definitely/not/here"));
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("not/here"));
}

#[test]
fn unreadable_files_are_named_on_stderr_and_the_rest_is_still_read() {
    let s = Scratch::new();
    s.file("note.md", "x\n")
        .file("crlf.md", "a\r\n[[note]]\r\n")
        .file("bom.md", [&[0xEF, 0xBB, 0xBF][..], b"[[note]]\n"].concat())
        .file("empty.md", "")
        .file("bad.md", [0xFFu8, 0xFE, 0x00, 0x41]);

    let graph = textree(&["graph", "--json"], s.path());
    assert_eq!(graph.status.code(), Some(0));
    assert!(stderr(&graph).contains("bad.md"));
    let json: serde_json::Value = serde_json::from_str(&stdout(&graph)).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|l| l["to"] == "note.md"));
    let crlf = links.iter().find(|l| l["from"] == "crlf.md").unwrap();
    assert_eq!(crlf["line"], 2);
    assert!(json["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["path"] == "empty.md"));

    let check = textree(&["check"], s.path());
    assert_eq!(check.status.code(), Some(1));
    assert!(stderr(&check).contains("bad.md"));
}

#[test]
fn json_output_is_identical_across_runs_creation_order_and_path_spelling() {
    let first = textree(&["graph", "--json"], &corpus_tree());
    let second = textree(&["graph", "--json"], &corpus_tree());
    assert_eq!(first.stdout, second.stdout);

    // Copy the tree, creating files in reverse order.
    let s = Scratch::new();
    let mut files = Vec::new();
    collect(&corpus_tree(), &corpus_tree(), &mut files);
    files.sort();
    files.reverse();
    for rel in &files {
        s.file(rel, fs::read(corpus_tree().join(rel)).unwrap());
    }
    let copied = textree(&["graph", "--json"], s.path());
    assert_eq!(first.stdout, copied.stdout);

    // A relative path from another working directory gives the same output.
    let relative = Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(["graph", "--json", "tests/data/corpus/tree"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    assert_eq!(first.stdout, relative.stdout);
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            out.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn scanning_never_writes_to_the_folder() {
    let s = Scratch::new();
    s.file("a.md", "[[b]] #t\n")
        .file("b.md", "x\n")
        .file("img/p.png", [1u8, 2, 3]);
    let before = snapshot(s.path());
    textree(&["graph"], s.path());
    textree(&["graph", "--json"], s.path());
    textree(&["check"], s.path());
    assert_eq!(before, snapshot(s.path()));
}

fn snapshot(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64, std::time::SystemTime)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::metadata(&path).unwrap();
            out.push((
                path.strip_prefix(root).unwrap().display().to_string(),
                meta.len(),
                meta.modified().unwrap(),
            ));
            if path.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn the_summary_names_the_choice_made_between_two_documents_of_one_name() {
    let out = textree(&["graph"], &corpus_tree());
    let text = stdout(&out);
    assert!(text.contains("8 documents (1 asset)"), "{text}");
    assert!(text.contains("links    6 resolved · 0 broken"), "{text}");
    assert!(
        text.contains("note: 2 documents are named \"soil\"."),
        "{text}"
    );
    assert!(
        text.contains("-> projects/garden/soil.md (same folder)"),
        "{text}"
    );
}
