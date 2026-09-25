//! Gate: a write interrupted at any step leaves the note as it was.
//!
//! An atomic write has four steps — make a temp file, write into it, flush it to storage, rename
//! it onto the note. Two kinds of interruption are checked before each one, over a table of folder
//! shapes rather than a single case:
//!
//! - **The step fails** (a full disk, a lock, a folder that stops answering): the write reports
//!   the failure, the note still holds its old bytes, and nothing is left behind.
//! - **Everything stops at that instant** (a crash, a power cut — no cleanup runs): the disk is
//!   copied as it stands at that moment, and the copy must hold the note's old bytes, may hold
//!   one temp file only where the application stages them, and — once the sweep that runs when a
//!   folder is opened has been through — must hold exactly what it held before the write began.
//!
//! Negative controls at the bottom show the check is falsifiable.

#![cfg(test)]

use crate::commands::{
    atomic_write_bytes, clear_temp_dir, write_step, LOOSE_TEMP_PREFIX, TEMP_DIR_NAME,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tempfile::TempDir;
use write_step::Step;

const STEPS: [Step; 4] = [Step::CreateTemp, Step::WriteTemp, Step::Sync, Step::Rename];

#[derive(Clone, Copy)]
enum Repo {
    /// A plain folder — temp files stage at its top, under the application's own prefix.
    None,
    /// The folder is a repository — temp files stage in its storage.
    Here,
    /// The folder sits inside someone's repository — temp files stage in that repository's
    /// storage, never in the folder.
    Above,
}

struct Shape {
    name: &'static str,
    repo: Repo,
    /// The opened folder, relative to the directory the shape is built in.
    vault: &'static str,
    /// Files present before the write, relative to the directory the shape is built in.
    files: &'static [(&'static str, &'static [u8])],
    target: &'static str,
    new: &'static [u8],
}

const AFTER: &[u8] = b"# after\n\nlonger than what was there before\n";

const SHAPES: &[Shape] = &[
    Shape {
        name: "a note in a plain folder",
        repo: Repo::None,
        vault: "",
        files: &[("a.md", b"# before\n")],
        target: "a.md",
        new: AFTER,
    },
    Shape {
        name: "a new note in a plain folder",
        repo: Repo::None,
        vault: "",
        files: &[("other.md", b"# other\n")],
        target: "new.md",
        new: AFTER,
    },
    Shape {
        name: "a note in a subfolder of a plain folder",
        repo: Repo::None,
        vault: "",
        files: &[("sub/a.md", b"# before\n")],
        target: "sub/a.md",
        new: AFTER,
    },
    Shape {
        name: "a note in a repository",
        repo: Repo::Here,
        vault: "",
        files: &[("a.md", b"# before\n")],
        target: "a.md",
        new: AFTER,
    },
    Shape {
        name: "a new note in a subfolder of a repository",
        repo: Repo::Here,
        vault: "",
        files: &[("sub/other.md", b"# other\n")],
        target: "sub/new.md",
        new: AFTER,
    },
    Shape {
        name: "a note in a folder inside someone's repository",
        repo: Repo::Above,
        vault: "docs",
        files: &[("docs/a.md", b"# before\n"), ("src/main.rs", b"fn main() {}\n")],
        target: "docs/a.md",
        new: AFTER,
    },
    Shape {
        name: "a file that is not text",
        repo: Repo::Here,
        vault: "",
        files: &[("image.bin", &[0xff, 0x00, 0xfe, 0x01])],
        target: "image.bin",
        new: &[0x00, 0xff, 0x00, 0xff, 0x80],
    },
];

/// Every file under `dir`, by path relative to it with `/` separators.
type Disk = BTreeMap<String, Vec<u8>>;

fn read_disk(dir: &Path) -> Disk {
    fn walk(base: &Path, dir: &Path, out: &mut Disk) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = Disk::new();
    walk(dir, dir, &mut out);
    out
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn build(shape: &Shape) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();
    if !matches!(shape.repo, Repo::None) {
        git2::Repository::init(base).unwrap();
    }
    std::fs::create_dir_all(base.join(shape.vault)).unwrap();
    for (rel, body) in shape.files {
        let path = base.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    tmp
}

fn is_repository_storage(rel: &str) -> bool {
    rel == ".git" || rel.starts_with(".git/")
}

/// A place the application stages temp files: its directory in repository storage, or the top
/// of the opened folder under its own prefix.
fn is_staged_temp(rel: &str, vault: &str) -> bool {
    if let Some(name) = rel.strip_prefix(&format!(".git/{TEMP_DIR_NAME}/")) {
        return !name.contains('/');
    }
    let top = if vault.is_empty() { rel.to_string() } else {
        match rel.strip_prefix(&format!("{vault}/")) {
            Some(rest) => rest.to_string(),
            None => return false,
        }
    };
    !top.contains('/') && top.starts_with(LOOSE_TEMP_PREFIX)
}

/// What the disk may look like if everything stopped before `step`: the note untouched, nothing
/// that was there gone or changed, at most the one temp the write is building — and after the
/// sweep a folder open runs, exactly the folder as it was.
///
/// `image` is the directory the shape was built in (or a copy of it); `before` is that directory
/// read before the write began.
fn check_crash_image(
    image: &Path,
    shape: &Shape,
    before: &Disk,
    step: Step,
) -> Result<(), String> {
    let now = read_disk(image);

    if now.get(shape.target) != before.get(shape.target) {
        return Err(format!("the note '{}' changed before the write finished", shape.target));
    }

    let mut temps = Vec::new();
    for (rel, bytes) in &now {
        match before.get(rel) {
            Some(old) if old == bytes => {}
            Some(_) if is_repository_storage(rel) && !rel.starts_with(".git/objects/") => {}
            Some(_) => return Err(format!("'{rel}' changed")),
            None if is_staged_temp(rel, shape.vault) => temps.push((rel.clone(), bytes.clone())),
            None if is_repository_storage(rel) => {}
            None => return Err(format!("'{rel}' appeared where temp files never stage")),
        }
    }
    if let Some(gone) = before.keys().find(|rel| !now.contains_key(*rel)) {
        return Err(format!("'{gone}' disappeared"));
    }

    let expected = if step == Step::CreateTemp { 0 } else { 1 };
    if temps.len() != expected {
        return Err(format!("{} temp files staged before {step:?}, expected {expected}", temps.len()));
    }
    if matches!(step, Step::Sync | Step::Rename) && temps[0].1 != shape.new {
        return Err(format!("the temp '{}' does not hold what is being written", temps[0].0));
    }

    clear_temp_dir(&image.join(shape.vault));
    let swept = read_disk(image);
    let outside_storage = |disk: &Disk| -> Disk {
        disk.iter()
            .filter(|(rel, _)| !is_repository_storage(rel))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    if outside_storage(&swept) != outside_storage(before) {
        return Err("after the sweep the folder is not what it was before the write".into());
    }
    if let Some(left) = swept.keys().find(|rel| is_staged_temp(rel, shape.vault)) {
        return Err(format!("the sweep left '{left}'"));
    }
    Ok(())
}

#[test]
fn everything_stopping_before_any_step_leaves_the_note_as_it_was() {
    for shape in SHAPES {
        let tmp = build(shape);
        let base = tmp.path().to_path_buf();
        let before = read_disk(&base);

        // Each image is the disk exactly as a crash before that step would leave it: a copy
        // taken while the write is suspended, with no cleanup of any kind run.
        let images: Rc<RefCell<Vec<(Step, TempDir)>>> = Rc::default();
        let _hook = write_step::install({
            let images = Rc::clone(&images);
            let base = base.clone();
            move |step| {
                let image = TempDir::new().unwrap();
                copy_tree(&base, image.path());
                images.borrow_mut().push((step, image));
                Ok(())
            }
        });

        atomic_write_bytes(&base.join(shape.vault), &base.join(shape.target), shape.new)
            .unwrap_or_else(|e| panic!("{}: the write itself failed: {e}", shape.name));

        let images = images.borrow();
        let seen: Vec<Step> = images.iter().map(|(s, _)| *s).collect();
        assert_eq!(seen, STEPS, "{}: every step is reached, in order", shape.name);
        for (step, image) in images.iter() {
            check_crash_image(image.path(), shape, &before, *step)
                .unwrap_or_else(|e| panic!("{}, stopped before {step:?}: {e}", shape.name));
        }

        assert_eq!(
            std::fs::read(base.join(shape.target)).unwrap(),
            shape.new,
            "{}: the uninterrupted write lands",
            shape.name
        );
    }
}

#[test]
fn a_step_that_fails_leaves_the_note_as_it_was_and_nothing_behind() {
    for shape in SHAPES {
        for failing in STEPS {
            let tmp = build(shape);
            let base = tmp.path().to_path_buf();
            let before = read_disk(&base);

            let _hook = write_step::install(move |step| {
                if step == failing {
                    Err(io::Error::other("injected"))
                } else {
                    Ok(())
                }
            });
            let result =
                atomic_write_bytes(&base.join(shape.vault), &base.join(shape.target), shape.new);

            assert!(result.is_err(), "{}, failing at {failing:?}: the failure is reported", shape.name);
            let after = read_disk(&base);
            assert_eq!(
                after.get(shape.target),
                before.get(shape.target),
                "{}, failing at {failing:?}: the note keeps its bytes",
                shape.name
            );
            let left: Vec<&String> = after
                .keys()
                .filter(|rel| {
                    (!before.contains_key(*rel) && !is_repository_storage(rel))
                        || is_staged_temp(rel, shape.vault)
                })
                .collect();
            assert!(left.is_empty(), "{}, failing at {failing:?}: left behind {left:?}", shape.name);
        }
    }
}

// --- negative controls: the check can fail ---

fn shape_named(name: &str) -> &'static Shape {
    SHAPES.iter().find(|s| s.name == name).unwrap()
}

#[test]
fn negative_control_writing_straight_into_the_note_is_caught() {
    // Truncate-then-write, the way a plain `write` works: stopping in between leaves the note
    // empty.
    let shape = shape_named("a note in a plain folder");
    let tmp = build(shape);
    let before = read_disk(tmp.path());

    std::fs::File::create(tmp.path().join(shape.target)).unwrap();

    let err = check_crash_image(tmp.path(), shape, &before, Step::WriteTemp).unwrap_err();
    assert!(err.contains("changed before the write finished"), "{err}");
}

#[test]
fn negative_control_a_temp_without_the_prefix_is_caught() {
    // Staged at the top of the folder but under a name the sweep does not recognize: it would
    // stay in someone's notes folder forever.
    let shape = shape_named("a note in a plain folder");
    let tmp = build(shape);
    let before = read_disk(tmp.path());

    std::fs::write(tmp.path().join(".tmpAbC123"), shape.new).unwrap();

    let err = check_crash_image(tmp.path(), shape, &before, Step::Sync).unwrap_err();
    assert!(err.contains("where temp files never stage"), "{err}");
}

#[test]
fn negative_control_a_temp_beside_the_note_is_caught() {
    // The application's prefix, but beside the note instead of at the top: the sweep only looks
    // at the top, so this one would never go.
    let shape = shape_named("a note in a subfolder of a plain folder");
    let tmp = build(shape);
    let before = read_disk(tmp.path());

    std::fs::write(tmp.path().join(format!("sub/{LOOSE_TEMP_PREFIX}x")), shape.new).unwrap();

    let err = check_crash_image(tmp.path(), shape, &before, Step::Sync).unwrap_err();
    assert!(err.contains("where temp files never stage"), "{err}");
}

#[test]
fn negative_control_a_missing_temp_is_caught() {
    // A write that claims to be past making its temp file but staged nothing is writing
    // somewhere the check cannot see.
    let shape = shape_named("a note in a repository");
    let tmp = build(shape);
    let before = read_disk(tmp.path());

    let err = check_crash_image(tmp.path(), shape, &before, Step::Rename).unwrap_err();
    assert!(err.contains("0 temp files"), "{err}");
}

#[test]
fn negative_control_the_shapes_are_built_where_they_claim() {
    // The folder-inside-a-repository shape only proves anything if the repository really is
    // above the opened folder.
    let shape = shape_named("a note in a folder inside someone's repository");
    let tmp = build(shape);
    let vault: PathBuf = tmp.path().join(shape.vault);
    assert!(!vault.join(".git").exists());
    assert!(tmp.path().join(".git").is_dir());
}
