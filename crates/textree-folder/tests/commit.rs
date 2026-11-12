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
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
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

#[test]
fn a_failed_verification_puts_everything_back() {
    let s = Scratch::new();
    busy(&s);
    let want = snapshot(s.path());
    let cs = plan_move(s.path(), "projects/garden", "archive/yard");
    let store = FolderStore::open(s.path()).unwrap();
    let err = store
        .commit_checked(&cs, &|_| {
            Err("the folder is not what the plan said".to_string())
        })
        .unwrap_err();
    match err {
        CommitError::Failed { message, .. } => {
            assert!(message.contains("not what the plan said"), "{message}");
            assert!(message.contains("put back"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(s.path()), want);
    // And a verification that agrees lets it through.
    store.commit_checked(&cs, &|_| Ok(())).unwrap();
}

#[cfg(unix)]
#[test]
fn nothing_is_written_through_a_symbolic_link() {
    let outside = Scratch::new();
    let s = Scratch::new();
    busy(&s);
    std::os::unix::fs::symlink(outside.path(), s.path().join("lnk")).unwrap();
    let store = FolderStore::open(s.path()).unwrap();

    // A plan that moves into the linked folder.
    let cs = plan_move(s.path(), "solo/only.md", "lnk/only.md");
    let err = store.preflight(&cs).unwrap_err();
    assert!(
        matches!(&err, CommitError::Invalid(why) if why.contains("symbolic link")),
        "{err:?}"
    );
    assert!(matches!(store.commit(&cs), Err(CommitError::Invalid(_))));
    assert!(s.path().join("solo/only.md").exists());
    assert!(!outside.path().join("only.md").exists());

    // A hand-edited plan that writes through it.
    let mut forged = plan_move(s.path(), "solo/only.md", "solo/other.md");
    forged.writes.push(textree::Write {
        path: "lnk/victim.md".into(),
        content: "PWNED".into(),
        note: None,
    });
    assert!(matches!(
        store.commit(&forged),
        Err(CommitError::Invalid(_))
    ));
    assert!(!outside.path().join("victim.md").exists());
}

fn plan_split(root: &Path, path: &str, level: u8) -> ChangeSet {
    let store = FolderStore::open(root).unwrap();
    plan(
        &Snapshot::read(&store),
        &Op::Split {
            path: path.into(),
            level,
        },
        &Policy::default(),
    )
    .unwrap()
    .changes
}

fn trip(s: &Scratch) {
    s.file(
        "notes/trip.md",
        "# Trip\n\nIntro [[sleep]].\n\n## Day 1\n\nWalk [[sleep]].\n\n## Day 2\n\nRest.\n",
    )
    .file("notes/sleep.md", "z\n")
    .file("index.md", "[t](notes/trip.md#day-2) [[trip#Day 1]]\n");
}

#[test]
fn a_failure_while_splitting_puts_everything_back() {
    let reference = Scratch::new();
    trip(&reference);
    let want = snapshot(reference.path());
    let mut failures = 0;
    for n in 1..=60 {
        let s = Scratch::new();
        trip(&s);
        let cs = plan_split(s.path(), "notes/trip.md", 2);
        assert_eq!(cs.creates.len(), 2);
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
                    "failure at step {n} left the folder changed (a created file or folder, or a \
                     temporary trace)"
                );
            }
            Ok(_) => {
                let (files, _) = snapshot(s.path());
                assert!(files.contains_key("notes/trip/day-1.md"));
                assert!(!files.contains_key("notes/trip.md"));
                break;
            }
            Err(other) => panic!("step {n}: {other:?}"),
        }
    }
    assert!(failures >= 8, "only {failures} steps could be made to fail");
}

#[test]
fn a_created_document_never_replaces_a_file_that_appeared_after_the_plan() {
    let s = Scratch::new();
    trip(&s);
    let cs = plan_split(s.path(), "notes/trip.md", 2);
    // Someone creates the file the plan was going to create. The listing changes too, so the
    // plan is stale; either way nothing may be overwritten.
    s.file("notes/trip/day-1.md", "theirs\n");
    let store = FolderStore::open(s.path()).unwrap();
    let outcome = store.commit(&cs);
    assert!(
        matches!(
            outcome,
            Err(CommitError::Stale(_)) | Err(CommitError::Conflict(_))
        ),
        "{outcome:?}"
    );
    assert_eq!(
        fs::read_to_string(s.path().join("notes/trip/day-1.md")).unwrap(),
        "theirs\n"
    );
    assert!(s.path().join("notes/trip.md").exists());
}

#[cfg(unix)]
#[test]
fn a_create_over_a_name_the_listing_cannot_see_is_a_conflict_not_an_overwrite() {
    // A dangling link is not a document, so it cannot make the plan stale; the commit itself
    // has to refuse to write through or over it.
    let s = Scratch::new();
    s.file("a.md", "# A\n\n## One\n\n1\n");
    let cs = plan_split(s.path(), "a.md", 2);
    fs::create_dir_all(s.path().join("a")).unwrap();
    std::os::unix::fs::symlink("nowhere.md", s.path().join("a/one.md")).unwrap();
    let store = FolderStore::open(s.path()).unwrap();
    let outcome = store.commit(&cs);
    assert!(
        matches!(
            outcome,
            Err(CommitError::Conflict(_)) | Err(CommitError::Stale(_))
        ),
        "{outcome:?}"
    );
    assert!(!s.path().join("nowhere.md").exists());
    assert!(s.path().join("a.md").exists());
}

#[test]
fn a_forged_create_may_not_leave_the_folder() {
    let s = Scratch::new();
    s.file("a.md", "# A\n");
    let outside = Scratch::new();
    let store = FolderStore::open(s.path()).unwrap();
    let mut cs = plan_move(s.path(), "a.md", "b.md");
    for bad in ["../escape.md", "/abs.md", ".hidden/x.md", "ok/../../x.md"] {
        cs.creates = vec![textree::Write {
            path: bad.into(),
            content: "PWNED".into(),
            note: None,
        }];
        assert!(
            matches!(store.commit(&cs), Err(CommitError::Invalid(_))),
            "{bad}"
        );
    }
    assert!(!outside.path().join("escape.md").exists());
    assert!(s.path().join("a.md").exists());
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_that_leads_outside_the_folder_is_not_part_of_it() {
    use textree::ReadPort;
    let s = Scratch::new();
    s.file("a.md", "# A\n").file("sub/b.md", "# B\n");
    let outside = Scratch::new();
    outside.file("secret.md", "# Secret\n");
    std::os::unix::fs::symlink(outside.path().join("secret.md"), s.path().join("out.md")).unwrap();
    std::os::unix::fs::symlink(outside.path(), s.path().join("outdir")).unwrap();
    std::os::unix::fs::symlink(s.path().join("sub/b.md"), s.path().join("in.md")).unwrap();
    let store = FolderStore::open(s.path()).unwrap();
    let mut listed: Vec<String> = store.list().nodes.into_iter().map(|n| n.path).collect();
    listed.sort();
    assert_eq!(listed, ["a.md", "in.md", "sub", "sub/b.md"]);
    assert!(store.read("out.md").is_err());
    assert!(store.read("outdir/secret.md").is_err());
    assert_eq!(store.read("in.md").unwrap(), b"# B\n");
    // Nothing can be planned on it.
    let snap = Snapshot::read(&store);
    for op in [
        Op::Delete {
            path: "out.md".into(),
        },
        Op::Move {
            from: "out.md".into(),
            to: "x.md".into(),
        },
    ] {
        assert!(plan(&snap, &op, &Policy::default()).is_err(), "{op:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_destination_taken_by_a_link_outside_the_folder_is_refused_when_planned() {
    use textree_folder::edit::{plan_change, FailureKind};
    let s = Scratch::new();
    s.file("b.md", "# B\n").file("sub/a.md", "# A\n");
    let outside = Scratch::new();
    outside.file("secret.md", "# Secret\n");
    std::os::unix::fs::symlink(outside.path().join("secret.md"), s.path().join("link.md")).unwrap();
    std::os::unix::fs::symlink(outside.path(), s.path().join("out")).unwrap();
    let store = FolderStore::open(s.path()).unwrap();
    for (op, at) in [
        (
            Op::Move {
                from: "b.md".into(),
                to: "link.md".into(),
            },
            "link.md",
        ),
        (
            Op::Add {
                path: "link.md".into(),
                content: "# New\n".into(),
            },
            "link.md",
        ),
        (
            Op::Move {
                from: "sub".into(),
                to: "out".into(),
            },
            "out",
        ),
    ] {
        let failure = plan_change(&store, &op, &Policy::default()).unwrap_err();
        assert_eq!(failure.kind, FailureKind::Refused, "{op:?}");
        assert_eq!(
            failure.lines,
            [format!("refused [I5] {at}: already exists")],
            "{op:?}"
        );
    }
    assert_eq!(
        fs::read(outside.path().join("secret.md")).unwrap(),
        b"# Secret\n"
    );
}
