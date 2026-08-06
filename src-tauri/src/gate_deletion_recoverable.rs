//! Gate: deleting something that was never recorded always leaves a way back.
//!
//! Recorded notes need no safety net — history holds them. Notes that were never recorded
//! have nothing behind them, so the deletion is the end of them unless their contents are kept
//! first. The invariant is *every* such file, not most of them, so this is stated over a table
//! of shapes rather than a single case.
//!
//! A negative control at the bottom shows the check is falsifiable.

#![cfg(test)]

use crate::commands::delete_node;
use crate::git_engine::{commit_identities, commit_paths, NOTES_REF, SNAPSHOT_REF};
use git2::Repository;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// One vault shape to delete something from: files to create, what to record first, and what
/// to delete.
struct Shape {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    recorded: &'static [&'static str],
    delete: &'static str,
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "a note that was never recorded",
        files: &[("a.md", "# a")],
        recorded: &[],
        delete: "a.md",
    },
    Shape {
        name: "a note inside a folder",
        files: &[("sub/a.md", "# a")],
        recorded: &[],
        delete: "sub/a.md",
    },
    Shape {
        name: "a whole folder of notes",
        files: &[("sub/a.md", "# a"), ("sub/b.md", "# b"), ("sub/deep/c.md", "# c")],
        recorded: &[],
        delete: "sub",
    },
    Shape {
        name: "a folder where only some notes were recorded",
        files: &[("sub/a.md", "# a"), ("sub/b.md", "# b")],
        recorded: &["sub/a.md"],
        delete: "sub",
    },
];

fn build(shape: &Shape) -> (TempDir, Repository) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();

    for (rel, body) in shape.files {
        let target = root.join(rel);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, body).unwrap();
    }

    if !shape.recorded.is_empty() {
        let entries: Vec<(PathBuf, Vec<u8>)> = shape
            .recorded
            .iter()
            .map(|rel| {
                (
                    PathBuf::from(rel),
                    std::fs::read(root.join(rel)).unwrap(),
                )
            })
            .collect();
        let (author, committer) = commit_identities(&repo).unwrap();
        commit_paths(&repo, NOTES_REF, &entries, "record", &author, &committer).unwrap();
    }

    (tmp, repo)
}

/// Contents kept for `rel`, if any.
fn kept(repo: &Repository, rel: &str) -> Option<Vec<u8>> {
    let tree = repo
        .find_reference(SNAPSHOT_REF)
        .ok()?
        .peel_to_commit()
        .ok()?
        .tree()
        .ok()?;
    let entry = tree.get_path(Path::new(rel)).ok()?;
    Some(repo.find_blob(entry.id()).ok()?.content().to_vec())
}

#[test]
fn every_unrecorded_file_deleted_is_still_recoverable() {
    for shape in SHAPES {
        let (tmp, repo) = build(shape);
        let root = tmp.path();

        delete_node(
            root.to_string_lossy().to_string(),
            root.join(shape.delete).to_string_lossy().to_string(),
        )
        .unwrap_or_else(|e| panic!("{}: deleting failed: {e}", shape.name));

        for (rel, body) in shape.files {
            let was_under_delete =
                *rel == shape.delete || rel.starts_with(&format!("{}/", shape.delete));
            if !was_under_delete {
                continue;
            }
            if shape.recorded.contains(rel) {
                // History already holds it; a second copy would be noise.
                continue;
            }
            assert_eq!(
                kept(&repo, rel).as_deref(),
                Some(body.as_bytes()),
                "{}: '{rel}' was deleted with nothing behind it",
                shape.name
            );
        }
    }
}

#[test]
fn deleting_something_already_recorded_keeps_no_second_copy() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();
    std::fs::write(root.join("a.md"), "# a").unwrap();

    let (author, committer) = commit_identities(&repo).unwrap();
    commit_paths(
        &repo,
        NOTES_REF,
        &[(PathBuf::from("a.md"), b"# a".to_vec())],
        "record",
        &author,
        &committer,
    )
    .unwrap();

    delete_node(
        root.to_string_lossy().to_string(),
        root.join("a.md").to_string_lossy().to_string(),
    )
    .unwrap();

    assert!(
        repo.find_reference(SNAPSHOT_REF).is_err(),
        "history already holds it, so nothing needs keeping"
    );
}

#[test]
fn negative_control_deleting_without_keeping_anything_loses_it() {
    // Establishes that the gate is not guarding against nothing: remove the same file the
    // plain way and its contents exist nowhere afterwards.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let repo = Repository::init(root).unwrap();
    std::fs::write(root.join("a.md"), "# a").unwrap();

    std::fs::remove_file(root.join("a.md")).unwrap();

    assert!(kept(&repo, "a.md").is_none());
    assert!(!root.join("a.md").exists());
}
