//! Low-level git operations that never go through the index or the working tree.
//!
//! Everything here addresses objects directly, so a write can target one path inside a
//! repository without disturbing what the user has staged or checked out.
//!
//! These primitives are covered by their own tests but have no caller yet; the command layer
//! starts using them when commits are wired up.
#![allow(dead_code)]

use git2::{Oid, Repository, Signature, Tree};
use std::path::{Component, Path, PathBuf};

const MODE_BLOB: i32 = 0o100644;
const MODE_TREE: i32 = 0o040000;

/// Builds a new tree from `base` with `rel` replaced by `blob` (or removed when `blob` is
/// `None`). Neither the index nor the working tree is read or written.
///
/// Only plain name components of `rel` are used; anything else (traversal prefixes, absolute
/// roots, drive prefixes) is dropped, so the result can never address a location outside the
/// tree. A path with no plain name component at all is an error.
pub fn tree_with_file(
    repo: &Repository,
    base: Option<&Tree<'_>>,
    rel: &Path,
    blob: Option<Oid>,
) -> Result<Oid, git2::Error> {
    let mut comps: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(str::to_string),
            _ => None,
        })
        .collect();
    if comps.is_empty() {
        return Err(git2::Error::from_str("path has no usable name component"));
    }
    let name = comps.remove(0);
    let mut builder = repo.treebuilder(base)?;

    if comps.is_empty() {
        match blob {
            Some(oid) => {
                builder.insert(name.as_str(), oid, MODE_BLOB)?;
            }
            None => builder.remove(name.as_str())?,
        }
        return builder.write();
    }

    let sub_base = base
        .and_then(|t| t.get_name(&name))
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.into_tree().ok());
    let rest: PathBuf = comps.iter().collect();
    let sub_oid = tree_with_file(repo, sub_base.as_ref(), &rest, blob)?;
    builder.insert(name.as_str(), sub_oid, MODE_TREE)?;
    builder.write()
}

/// Commits `content` at `rel` onto `reference`, carrying every other path in that reference's
/// tree forward unchanged. The index, the working tree and `HEAD` are all left alone, so this
/// is safe to run inside a repository someone else is working in.
///
/// `reference` is a full name such as `refs/heads/main`, and is created when absent.
pub fn commit_file(
    repo: &Repository,
    reference: &str,
    rel: &Path,
    content: &[u8],
    message: &str,
    author: &Signature<'_>,
) -> Result<Oid, git2::Error> {
    let blob = repo.blob(content)?;

    let parent = repo
        .find_reference(reference)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let base_tree = match parent.as_ref() {
        Some(commit) => Some(commit.tree()?),
        None => None,
    };

    let tree_oid = tree_with_file(repo, base_tree.as_ref(), rel, Some(blob))?;
    let tree = repo.find_tree(tree_oid)?;

    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    // Passing `None` as the update target keeps libgit2 from moving any reference on our
    // behalf: only the reference named by the caller is advanced, below.
    let commit = repo.commit(None, author, author, message, &tree, &parents)?;
    repo.reference(reference, commit, true, message)?;
    Ok(commit)
}

/// How a folder relates to whatever repository governs it. Purely observational: `probe`
/// creates nothing and modifies nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    /// The folder is itself the root of a repository.
    AtRoot,
    /// The folder sits inside a repository whose working directory is the given path.
    NestedIn(PathBuf),
    /// A bare repository was found at the given git directory. It has no working tree, so
    /// there is nothing for the folder to be the root of.
    Bare(PathBuf),
    /// No repository governs this folder.
    None,
}

/// Classifies `root` against the repository, if any, that contains it.
pub fn probe(root: &Path) -> RepoState {
    let Ok(repo) = Repository::discover(root) else {
        return RepoState::None;
    };
    let Some(workdir) = repo.workdir().and_then(|w| std::fs::canonicalize(w).ok()) else {
        return RepoState::Bare(repo.path().to_path_buf());
    };
    // Both sides are canonicalised so they carry the same path prefix form; comparing a
    // canonical path against a raw one mismatches on Windows extended-length prefixes.
    match std::fs::canonicalize(root) {
        Ok(canonical) if canonical == workdir => RepoState::AtRoot,
        Ok(_) => RepoState::NestedIn(workdir),
        Err(_) => RepoState::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use std::path::Path;
    use tempfile::TempDir;

    fn author() -> Signature<'static> {
        Signature::now("Test", "test@example.invalid").unwrap()
    }

    #[test]
    fn a_repository_can_be_created_and_reopened() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        let reopened = Repository::open(dir.path()).unwrap();
        assert!(reopened.workdir().is_some());
    }

    #[test]
    fn a_nested_path_commit_leaves_the_index_and_working_tree_untouched() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Something the user staged themselves. It must survive the commit untouched.
        std::fs::write(dir.path().join("staged.txt"), "user work").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        let entries_before = index.len();

        commit_file(
            &repo,
            "refs/heads/notes",
            Path::new("docs/site/a.md"),
            b"# heading",
            "add a note",
            &author(),
        )
        .unwrap();

        let tree = repo
            .find_reference("refs/heads/notes")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .tree()
            .unwrap();

        assert!(
            tree.get_path(Path::new("docs/site/a.md")).is_ok(),
            "the nested path must be in the commit"
        );
        assert!(
            tree.get_path(Path::new("staged.txt")).is_err(),
            "a staged file must not be swept into the commit"
        );

        let index_after = repo.index().unwrap();
        assert_eq!(
            index_after.len(),
            entries_before,
            "the index must be unchanged"
        );
        assert!(index_after.get_path(Path::new("staged.txt"), 0).is_some());

        assert!(
            !dir.path().join("docs").exists(),
            "nothing may be materialised on disk"
        );
    }

    #[test]
    fn committing_to_the_checked_out_branch_still_leaves_the_index_untouched() {
        // Pins the hardest case for the no-side-effects property: the target reference is the
        // branch HEAD points at, and the user has unrelated work staged on it. This fixes the
        // safety guarantee, not a policy about which reference a caller should target.
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("start.md"),
            b"start",
            "start",
            &author(),
        )
        .unwrap();
        repo.set_head("refs/heads/main").unwrap();

        std::fs::write(dir.path().join("staged.txt"), "user work").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        let entries_before = index.len();

        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("docs/b.md"),
            b"# second",
            "add a second note",
            &author(),
        )
        .unwrap();

        let tree = repo.head().unwrap().peel_to_commit().unwrap().tree().unwrap();
        assert!(tree.get_path(Path::new("docs/b.md")).is_ok());
        assert!(
            tree.get_path(Path::new("start.md")).is_ok(),
            "earlier content on the branch must be carried forward"
        );
        assert!(
            tree.get_path(Path::new("staged.txt")).is_err(),
            "a staged file must not be swept into the commit"
        );

        let index_after = repo.index().unwrap();
        assert_eq!(index_after.len(), entries_before);
        assert!(index_after.get_path(Path::new("staged.txt"), 0).is_some());
        assert!(!dir.path().join("docs").exists());
    }

    #[test]
    fn negative_control_an_index_based_commit_sweeps_staged_files() {
        // Establishes that the failure the path-scoped commit avoids is real: building the
        // tree from the index carries whatever the user had staged.
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        std::fs::write(dir.path().join("staged.txt"), "user work").unwrap();
        std::fs::write(dir.path().join("a.md"), "# heading").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.add_path(Path::new("a.md")).unwrap();
        let tree_oid = index.write_tree().unwrap();

        let tree = repo.find_tree(tree_oid).unwrap();
        assert!(
            tree.get_path(Path::new("staged.txt")).is_ok(),
            "the naive approach is expected to sweep the staged file in"
        );
    }

    #[test]
    fn a_blob_can_be_read_from_a_ref_without_checking_it_out() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("start.md"),
            b"start",
            "start",
            &author(),
        )
        .unwrap();
        repo.set_head("refs/heads/main").unwrap();
        let head_before = repo.head().unwrap().target();

        commit_file(
            &repo,
            "refs/heads/side",
            Path::new("note.md"),
            b"side content",
            "add a note on a side ref",
            &author(),
        )
        .unwrap();

        let tree = repo
            .find_reference("refs/heads/side")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .tree()
            .unwrap();
        let entry = tree.get_path(Path::new("note.md")).unwrap();
        let blob = repo.find_blob(entry.id()).unwrap();

        assert_eq!(blob.content(), b"side content");
        assert!(
            !dir.path().join("note.md").exists(),
            "no checkout may happen"
        );
        assert_eq!(
            repo.head().unwrap().target(),
            head_before,
            "HEAD must not move"
        );
    }

    #[test]
    fn path_components_that_are_not_plain_names_are_dropped() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let blob = repo.blob(b"x").unwrap();

        // A path made only of non-name components addresses nothing.
        assert!(tree_with_file(&repo, None, Path::new(".."), Some(blob)).is_err());

        // Traversal prefixes are dropped rather than followed: the entry lands at the
        // remaining plain name, never outside the tree.
        let oid = tree_with_file(&repo, None, Path::new("../escaped.md"), Some(blob)).unwrap();
        let tree = repo.find_tree(oid).unwrap();
        assert!(tree.get_path(Path::new("escaped.md")).is_ok());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn a_plain_folder_is_reported_as_having_no_repository() {
        let dir = TempDir::new().unwrap();
        let plain = dir.path().join("notes");
        std::fs::create_dir(&plain).unwrap();
        assert_eq!(probe(&plain), RepoState::None);
    }

    #[test]
    fn a_repository_root_is_reported_as_such() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        assert_eq!(probe(dir.path()), RepoState::AtRoot);
    }

    #[test]
    fn a_bare_repository_is_reported_as_bare_rather_than_as_a_root() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init_bare(dir.path()).unwrap();
        assert!(repo.workdir().is_none());

        match probe(dir.path()) {
            RepoState::Bare(gitdir) => {
                assert!(gitdir.exists(), "the reported git directory must exist");
            }
            other => panic!("expected a bare repository, got {other:?}"),
        }
    }

    #[test]
    fn a_folder_inside_a_repository_reports_the_enclosing_root() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        let inner = dir.path().join("docs").join("notes");
        std::fs::create_dir_all(&inner).unwrap();

        let expected = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(probe(&inner), RepoState::NestedIn(expected));
    }
}
