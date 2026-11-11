//! Committing change sets to a real folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use textree::{plan, ChangeSet, CommitError, Op, Policy, Snapshot, WritePort};
use textree_folder::FolderStore;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("textree-commit-test-{}-{n}", std::process::id()));
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
        // Make anything a test made read-only writable again, then remove.
        fn open_up(dir: &Path) {
            if let Ok(meta) = fs::metadata(dir) {
                let mut p = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                let _ = fs::set_permissions(dir, p);
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

/// Everything about a folder that matters: exact-case paths, bytes, and which folders exist.
fn snapshot(dir: &Path) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    fn walk(
        root: &Path,
        dir: &Path,
        files: &mut BTreeMap<String, Vec<u8>>,
        dirs: &mut Vec<String>,
    ) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
            if p.is_dir() {
                dirs.push(rel);
                walk(root, &p, files, dirs);
            } else {
                files.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    let mut dirs = Vec::new();
    walk(dir, dir, &mut files, &mut dirs);
    dirs.sort();
    (files, dirs)
}

fn plan_move(dir: &Path, from: &str, to: &str) -> ChangeSet {
    let store = FolderStore::open(dir).unwrap();
    plan(
        &Snapshot::read(&store),
        &Op::Move {
            from: from.into(),
            to: to.into(),
        },
        &Policy::default(),
    )
    .unwrap()
    .changes
}

/// A tree where one move moves a folder (and its folder document), rewrites links in several
/// documents and leaves a folder empty.
fn busy(s: &Scratch) {
    s.file(
        "projects/projects.md",
        "Garden: [[garden]] and [g](garden/garden.md).\n",
    )
    .file(
        "projects/garden/garden.md",
        "# Garden\n\n[w](watering.md) [up](../projects.md)\n",
    )
    .file("projects/garden/watering.md", "# Watering\n")
    .file("projects/garden/beds.png", [1u8, 2, 3])
    .file("notes/a.md", "[g](../projects/garden/garden.md)\n")
    .file(
        "notes/b.md",
        "[[garden]] [w](../projects/garden/watering.md)\n",
    )
    .file("solo/only.md", "[[garden]]\n");
}

#[test]
fn a_committed_plan_leaves_exactly_the_planned_tree_and_no_traces() {
    let s = Scratch::new();
    busy(&s);
    let cs = plan_move(s.path(), "projects/garden", "projects/yard");
    let store = FolderStore::open(s.path()).unwrap();
    store.commit(&cs).unwrap();
    let (files, dirs) = snapshot(s.path());
    assert!(files.contains_key("projects/yard/yard.md"));
    assert!(files.contains_key("projects/yard/beds.png"));
    assert!(!dirs.iter().any(|d| d.ends_with("garden")));
    // No temporary or hidden leftovers anywhere.
    assert!(
        files
            .keys()
            .chain(dirs.iter())
            .all(|p| !p.split('/').any(|piece| piece.starts_with('.'))),
        "leftovers: {:?}",
        files.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        String::from_utf8(files["projects/projects.md"].clone()).unwrap(),
        "Garden: [[yard]] and [g](yard/yard.md).\n"
    );
}

#[test]
fn a_failure_at_any_step_puts_everything_back() {
    let reference = Scratch::new();
    busy(&reference);
    let want = snapshot(reference.path());
    let mut failures = 0;
    for n in 1..=60 {
        let s = Scratch::new();
        busy(&s);
        let cs = plan_move(s.path(), "projects/garden", "archive/yard");
        let store = FolderStore::open(s.path()).unwrap().with_failure_at(n);
        match store.commit(&cs) {
            Err(CommitError::Failed { message, .. }) => {
                failures += 1;
                assert!(
                    message.contains("everything was put back"),
                    "step {n}: {message}"
                );
                assert_eq!(
                    snapshot(s.path()),
                    want,
                    "failure at step {n} left the folder changed"
                );
            }
            Ok(_) => break,
            Err(other) => panic!("step {n}: {other:?}"),
        }
    }
    assert!(failures >= 8, "only {failures} steps could be made to fail");
}

#[test]
fn a_failure_while_deleting_puts_the_documents_back() {
    let reference = Scratch::new();
    busy(&reference);
    let want = snapshot(reference.path());
    let mut failures = 0;
    for n in 1..=40 {
        let s = Scratch::new();
        busy(&s);
        let store = FolderStore::open(s.path()).unwrap().with_failure_at(n);
        let cs = plan(
            &Snapshot::read(&store),
            &Op::Delete {
                path: "notes".into(),
            },
            &Policy::default(),
        )
        .unwrap()
        .changes;
        match store.commit(&cs) {
            Err(CommitError::Failed { .. }) => {
                failures += 1;
                assert_eq!(snapshot(s.path()), want, "failure at step {n}");
            }
            Ok(_) => {
                let (files, dirs) = snapshot(s.path());
                assert!(!files.keys().any(|k| k.starts_with("notes/")));
                assert!(!dirs.contains(&"notes".to_string()), "folder left behind");
                break;
            }
            Err(other) => panic!("{other:?}"),
        }
    }
    assert!(failures >= 2);
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_written_aborts_and_restores() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    busy(&s);
    let want = snapshot(s.path());
    let cs = plan_move(s.path(), "projects/garden", "projects/yard");
    // `notes/` holds documents that must be rewritten; make it impossible to write there.
    fs::set_permissions(s.path().join("notes"), fs::Permissions::from_mode(0o555)).unwrap();
    let store = FolderStore::open(s.path()).unwrap();
    let err = store.commit(&cs).unwrap_err();
    fs::set_permissions(s.path().join("notes"), fs::Permissions::from_mode(0o755)).unwrap();
    match err {
        CommitError::Failed { path, .. } => assert!(path.starts_with("notes/"), "{path}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(s.path()), want);
}

#[test]
fn a_plan_is_stale_when_a_read_document_changed() {
    let s = Scratch::new();
    busy(&s);
    let cs = plan_move(s.path(), "projects/garden", "projects/yard");
    fs::write(s.path().join("notes/a.md"), "edited meanwhile\n").unwrap();
    let before = snapshot(s.path());
    let err = FolderStore::open(s.path())
        .unwrap()
        .commit(&cs)
        .unwrap_err();
    assert_eq!(err, CommitError::Stale(vec!["notes/a.md".to_string()]));
    assert_eq!(snapshot(s.path()), before);
}

#[test]
fn a_plan_is_stale_when_a_document_was_added_anywhere() {
    let s = Scratch::new();
    busy(&s);
    let cs = plan_move(s.path(), "projects/garden", "projects/yard");
    s.file("q/garden.md", "a second garden\n");
    let before = snapshot(s.path());
    let err = FolderStore::open(s.path())
        .unwrap()
        .commit(&cs)
        .unwrap_err();
    assert_eq!(err, CommitError::Stale(vec!["q/garden.md".to_string()]));
    assert_eq!(snapshot(s.path()), before);
}

#[test]
fn a_plan_cannot_be_applied_twice() {
    let s = Scratch::new();
    busy(&s);
    let cs = plan_move(s.path(), "projects/garden", "projects/yard");
    let store = FolderStore::open(s.path()).unwrap();
    store.commit(&cs).unwrap();
    assert!(matches!(store.commit(&cs), Err(CommitError::Stale(_))));
}

#[test]
fn a_plan_that_leaves_the_root_is_invalid() {
    let s = Scratch::new();
    busy(&s);
    let mut cs = plan_move(s.path(), "projects/garden", "projects/yard");
    cs.moves[0].to = "../escaped".to_string();
    let before = snapshot(s.path());
    let err = FolderStore::open(s.path())
        .unwrap()
        .commit(&cs)
        .unwrap_err();
    assert!(matches!(err, CommitError::Invalid(_)), "{err:?}");
    assert_eq!(snapshot(s.path()), before);
    let mut cs = plan_move(s.path(), "projects/garden", "projects/yard");
    cs.writes[0].path = "/etc/passwd".to_string();
    assert!(matches!(
        FolderStore::open(s.path()).unwrap().commit(&cs),
        Err(CommitError::Invalid(_))
    ));
}

#[test]
fn a_rename_that_only_changes_case_leaves_the_new_spelling_on_disk() {
    let s = Scratch::new();
    s.file("Note.md", "# N\n")
        .file("ref.md", "[a](Note.md) [[Note]]\n");
    let cs = plan_move(s.path(), "Note.md", "note.md");
    FolderStore::open(s.path()).unwrap().commit(&cs).unwrap();
    let (files, _) = snapshot(s.path());
    assert_eq!(
        files.keys().cloned().collect::<Vec<_>>(),
        vec!["note.md", "ref.md"]
    );
    assert_eq!(files["ref.md"], b"[a](note.md) [[Note]]\n");
}
