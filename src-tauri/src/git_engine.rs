//! Low-level git operations that never go through the index or the working tree.
//!
//! Everything here addresses objects directly, so a write can target one path inside a
//! repository without disturbing what the user has staged or checked out.
//!
//! Reads here are equally hands-off: looking at an earlier state addresses objects and never
//! materialises anything, so history can be browsed while the folder stays exactly as it is.
#![allow(dead_code)]

use git2::{Oid, Repository, RepositoryState, Signature, Tree};
use std::path::{Component, Path, PathBuf};

const MODE_BLOB: i32 = 0o100644;
const MODE_TREE: i32 = 0o040000;

/// The reference every note commit advances.
///
/// Deliberately outside `refs/heads/`: it never appears in branch listings, is never a
/// checkout target, and is unaffected by whatever the person working in the repository does
/// to their own branches. It is still an ordinary reference, so everything it holds is a
/// reachability root — garbage collection keeps it, and `git log --all` shows it.
pub const NOTES_REF: &str = "refs/textree/notes";

/// Where the contents of never-recorded files are kept when they are deleted.
///
/// A note that was never recorded has nothing in history to go back to, so deleting it would
/// otherwise be the end of it. This reference is not part of the history anyone reads; it
/// exists only so a deletion can be undone.
pub const SNAPSHOT_REF: &str = "refs/textree/snapshots";

/// A path as git spells it: plain name components joined by forward slashes.
///
/// Paths arriving from the rest of the application carry whatever separator the platform uses,
/// while everything stored in a tree is separated by `/`. Comparing the two forms directly
/// matches at the top level and silently fails one level down.
fn slashed(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Looks up a blob by walking `rel` one name at a time.
///
/// Resolving the whole path in one call would hand the platform's separator to a lookup that
/// only understands `/`; descending name by name sidesteps the question entirely, the same way
/// the tree builder does.
fn blob_in_tree(repo: &Repository, tree: &Tree<'_>, rel: &Path) -> Option<Oid> {
    let names: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let (last, dirs) = names.split_last()?;
    let mut current = tree.clone();
    for name in dirs {
        let next = current.get_name(name)?.to_object(repo).ok()?.into_tree().ok()?;
        current = next;
    }
    let entry = current.get_name(last)?;
    (entry.kind() == Some(git2::ObjectType::Blob)).then(|| entry.id())
}

/// The tree of the newest revision on `reference`, or `None` when the reference does not exist.
fn tip_tree<'r>(repo: &'r Repository, reference: &str) -> Option<Tree<'r>> {
    repo.find_reference(reference)
        .and_then(|r| r.peel_to_commit())
        .and_then(|c| c.tree())
        .ok()
}

/// Whether `reference` already holds something at `rel`.
pub fn is_recorded(repo: &Repository, reference: &str, rel: &Path) -> bool {
    tip_tree(repo, reference)
        .and_then(|t| blob_in_tree(repo, &t, rel))
        .is_some()
}

/// One recorded state of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedVersion {
    /// Identifies the revision. Opaque to callers; hand it back to [`content_at`].
    pub id: String,
    /// What the person recording it called this state.
    pub message: String,
    /// When it was recorded, in seconds since the epoch.
    pub seconds: i64,
    /// Who it is recorded as being written by.
    pub author: String,
}

/// Every path the newest revision on `reference` holds, separated by `/`.
///
/// Returns nothing when the reference does not exist, which is the ordinary state of a folder
/// where nothing has been recorded yet rather than a failure.
pub fn tip_paths(repo: &Repository, reference: &str) -> Result<Vec<String>, git2::Error> {
    let Some(tree) = tip_tree(repo, reference) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            if let Ok(name) = entry.name() {
                out.push(format!("{dir}{name}"));
            }
        }
        git2::TreeWalkResult::Ok
    })?;
    Ok(out)
}

/// The paths a revision changed relative to its first parent.
fn changed_paths(repo: &Repository, commit: &git2::Commit<'_>) -> Result<Vec<String>, git2::Error> {
    let tree = commit.tree()?;
    let parent = match commit.parent(0) {
        Ok(p) => Some(p.tree()?),
        Err(_) => None,
    };
    let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None)?;
    let mut out: Vec<String> = Vec::new();
    for delta in diff.deltas() {
        for path in [delta.new_file().path(), delta.old_file().path()]
            .into_iter()
            .flatten()
        {
            let s = slashed(path);
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        }
    }
    Ok(out)
}

/// Every revision on `reference` that changed `rel`, newest first.
///
/// Revisions that left `rel` alone are skipped: recording several notes at once would otherwise
/// make each of them appear to have changed whenever any of the others did.
pub fn history(
    repo: &Repository,
    reference: &str,
    rel: &Path,
) -> Result<Vec<RecordedVersion>, git2::Error> {
    let target = slashed(rel);
    let mut out = Vec::new();
    if repo.find_reference(reference).is_err() {
        return Ok(out);
    }
    let mut walk = repo.revwalk()?;
    // Topological as well as by time: several notes recorded in the same second carry
    // identical timestamps, and time alone leaves their order to chance. Topological order puts
    // a revision before the one it was built on regardless.
    walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
    walk.push_ref(reference)?;
    for id in walk {
        let commit = repo.find_commit(id?)?;
        if !changed_paths(repo, &commit)?.contains(&target) {
            continue;
        }
        out.push(RecordedVersion {
            id: commit.id().to_string(),
            message: commit.summary().ok().flatten().unwrap_or("").to_string(),
            seconds: commit.time().seconds(),
            author: commit.author().name().unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

/// When each path on `reference` was last changed, in seconds since the epoch.
///
/// Answered for every path at once because asking per path would re-read the whole history once
/// per note.
pub fn last_changed(
    repo: &Repository,
    reference: &str,
) -> Result<std::collections::HashMap<String, i64>, git2::Error> {
    let mut out = std::collections::HashMap::new();
    if repo.find_reference(reference).is_err() {
        return Ok(out);
    }
    let mut walk = repo.revwalk()?;
    // Topological as well as by time: several notes recorded in the same second carry
    // identical timestamps, and time alone leaves their order to chance. Topological order puts
    // a revision before the one it was built on regardless.
    walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
    walk.push_ref(reference)?;
    for id in walk {
        let commit = repo.find_commit(id?)?;
        let seconds = commit.time().seconds();
        for path in changed_paths(repo, &commit)? {
            // Newest first, so the first sighting of a path is its latest change.
            out.entry(path).or_insert(seconds);
        }
    }
    Ok(out)
}

/// What `rel` held at the given revision, if it held anything.
///
/// Reads objects only: nothing is written to the working tree, so looking at an earlier state
/// cannot disturb what is currently on disk.
pub fn content_at(
    repo: &Repository,
    id: &str,
    rel: &Path,
) -> Result<Option<Vec<u8>>, git2::Error> {
    let commit = repo.find_commit(Oid::from_str(id)?)?;
    let tree = commit.tree()?;
    let Some(blob) = blob_in_tree(repo, &tree, rel) else {
        return Ok(None);
    };
    Ok(Some(repo.find_blob(blob)?.content().to_vec()))
}

/// What `rel` holds in the newest revision on `reference`, if anything.
pub fn content_at_tip(
    repo: &Repository,
    reference: &str,
    rel: &Path,
) -> Result<Option<Vec<u8>>, git2::Error> {
    let Some(tree) = tip_tree(repo, reference) else {
        return Ok(None);
    };
    let Some(blob) = blob_in_tree(repo, &tree, rel) else {
        return Ok(None);
    };
    Ok(Some(repo.find_blob(blob)?.content().to_vec()))
}

/// Reports an operation the repository is in the middle of, if any.
///
/// Writing while a merge, rebase, bisect, revert, cherry-pick or mailbox application is in
/// flight interferes with state the other tool is about to act on. Any state other than a
/// clean one counts: the set of operations grows over time, and a state that has not been
/// enumerated here is exactly the one whose interaction is unknown.
pub fn operation_in_progress(repo: &Repository) -> Option<RepositoryState> {
    match repo.state() {
        RepositoryState::Clean => None,
        other => Some(other),
    }
}

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

/// Commits several paths onto `reference` as one revision, carrying every other path in that
/// reference's tree forward unchanged. The index, the working tree and `HEAD` are all left
/// alone, so this is safe to run inside a repository someone else is working in.
///
/// Grouping the paths into a single revision matters when a change spans files: committing
/// them one at a time publishes intermediate states in which links between them are broken.
///
/// `reference` is a full name such as `refs/heads/main`, and is created when absent.
pub fn commit_paths(
    repo: &Repository,
    reference: &str,
    entries: &[(PathBuf, Vec<u8>)],
    message: &str,
    author: &Signature<'_>,
    committer: &Signature<'_>,
) -> Result<Oid, git2::Error> {
    if entries.is_empty() {
        return Err(git2::Error::from_str("nothing to commit"));
    }

    let parent = repo
        .find_reference(reference)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let mut tree = match parent.as_ref() {
        Some(commit) => Some(commit.tree()?),
        None => None,
    };

    for (rel, content) in entries {
        let blob = repo.blob(content)?;
        let oid = tree_with_file(repo, tree.as_ref(), rel, Some(blob))?;
        tree = Some(repo.find_tree(oid)?);
    }
    let tree = tree.expect("entries is non-empty, so a tree was built");

    // Recording a state identical to the one already held would add a revision that changed
    // nothing — history would fill with entries a reader cannot tell apart.
    if let Some(commit) = parent.as_ref() {
        if commit.tree_id() == tree.id() {
            return Ok(commit.id());
        }
    }

    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    // Passing `None` as the update target keeps libgit2 from moving any reference on our
    // behalf: only the reference named by the caller is advanced, below.
    let commit = repo.commit(None, author, committer, message, &tree, &parents)?;
    repo.reference(reference, commit, true, message)?;
    Ok(commit)
}

/// Commits a single path. Convenience over [`commit_paths`] for callers with one file and no
/// separate committer.
pub fn commit_file(
    repo: &Repository,
    reference: &str,
    rel: &Path,
    content: &[u8],
    message: &str,
    author: &Signature<'_>,
) -> Result<Oid, git2::Error> {
    let entries = [(rel.to_path_buf(), content.to_vec())];
    commit_paths(repo, reference, &entries, message, author, author)
}

/// The name recorded as the committer of everything this application writes.
const APP_COMMITTER_NAME: &str = "Textree";
const APP_COMMITTER_EMAIL: &str = "noreply@textree.me";

/// Who a commit is recorded as being written by, and who recorded it.
///
/// The author is whoever the repository is configured for, so history stays theirs. Many
/// repositories have no identity configured at all — asking for one at this point would
/// interrupt saving a note, so the application's own identity stands in.
///
/// The committer is always the application, which makes every commit it wrote identifiable
/// without inspecting anything else.
pub fn commit_identities(
    repo: &Repository,
) -> Result<(Signature<'static>, Signature<'static>), git2::Error> {
    identities_from(repo.signature())
}

/// The identity rule itself, separated from where the configured identity comes from so the
/// fallback can be exercised without depending on the machine's git configuration.
fn identities_from(
    configured: Result<Signature<'static>, git2::Error>,
) -> Result<(Signature<'static>, Signature<'static>), git2::Error> {
    let app = || Signature::now(APP_COMMITTER_NAME, APP_COMMITTER_EMAIL);
    let author = configured.or_else(|_| app())?;
    Ok((author, app()?))
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

/// The storage directory of the repository governing `root`, if there is one.
///
/// Cheaper than opening the repository, and enough for callers that only need somewhere on the
/// same volume that is not part of anyone's working tree.
pub fn git_dir(root: &Path) -> Option<PathBuf> {
    Repository::discover(root).ok().map(|r| r.path().to_path_buf())
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

/// A repository together with where a vault folder sits inside it.
///
/// When the folder is itself a repository root the two coincide and the prefix is empty. When
/// it sits under a repository that already exists, that repository is used as-is and the
/// prefix records the distance, so writes land where the folder actually is.
pub struct VaultRepo {
    repo: Repository,
    prefix: PathBuf,
}

impl VaultRepo {
    pub fn repo(&self) -> &Repository {
        &self.repo
    }

    /// Translates a path relative to the vault folder into one relative to the repository.
    pub fn path_in_repo(&self, rel: &Path) -> PathBuf {
        if self.prefix.as_os_str().is_empty() {
            rel.to_path_buf()
        } else {
            self.prefix.join(rel)
        }
    }

    /// The reverse: a repository-relative path expressed relative to the vault folder.
    ///
    /// `None` when the path lies outside the folder, which is the ordinary case for everything
    /// else an enclosing repository holds.
    pub fn path_in_vault(&self, in_repo: &Path) -> Option<PathBuf> {
        if self.prefix.as_os_str().is_empty() {
            return Some(in_repo.to_path_buf());
        }
        in_repo.strip_prefix(&self.prefix).ok().map(Path::to_path_buf)
    }
}

/// Opens the repository that will hold a vault folder's history, creating one only when no
/// repository governs the folder yet.
///
/// A folder under an existing repository uses that repository rather than getting one of its
/// own: a repository inside a repository is recorded by the outer one as a link, so the outer
/// one's clones come out empty at that path, and staging everything breaks while the inner one
/// has no commit yet.
pub fn prepare(vault_root: &Path) -> Result<VaultRepo, git2::Error> {
    match probe(vault_root) {
        RepoState::AtRoot => Ok(VaultRepo {
            repo: Repository::open(vault_root)?,
            prefix: PathBuf::new(),
        }),
        RepoState::NestedIn(workdir) => {
            let canonical = std::fs::canonicalize(vault_root)
                .map_err(|e| git2::Error::from_str(&format!("cannot resolve the folder: {e}")))?;
            let prefix = canonical
                .strip_prefix(&workdir)
                .map_err(|_| {
                    git2::Error::from_str("the folder is not inside the repository that reported it")
                })?
                .to_path_buf();
            Ok(VaultRepo {
                repo: Repository::open(&workdir)?,
                prefix,
            })
        }
        // A bare repository is storage, not a place files live. Initialising another one on
        // top of its internals would layer a working tree over object storage, so this is
        // refused rather than guessed at.
        RepoState::Bare(_) => Err(git2::Error::from_str(
            "this folder is a repository without a working tree, so notes cannot live in it",
        )),
        RepoState::None => Ok(VaultRepo {
            repo: Repository::init(vault_root)?,
            prefix: PathBuf::new(),
        }),
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
    fn the_notes_reference_is_not_a_branch() {
        // Branch namespace is the user's. A reference outside it never shows up in branch
        // listings, is never a checkout target, and survives whatever they do to their own
        // branches.
        assert!(!NOTES_REF.starts_with("refs/heads/"));
        assert!(NOTES_REF.starts_with("refs/"));

        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        commit_file(
            &repo,
            NOTES_REF,
            Path::new("a.md"),
            b"# heading",
            "add a note",
            &author(),
        )
        .unwrap();

        let branches = repo.branches(None).unwrap().count();
        assert_eq!(branches, 0, "a note must not appear as a branch");
        assert!(repo.find_reference(NOTES_REF).is_ok());
    }

    #[test]
    fn committing_notes_never_moves_head() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // A repository someone else is working in: a branch exists and is checked out.
        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("app.txt"),
            b"their work",
            "their work",
            &author(),
        )
        .unwrap();
        repo.set_head("refs/heads/main").unwrap();
        let head_before = repo.head().unwrap().target().unwrap();

        for n in 0..5 {
            commit_file(
                &repo,
                NOTES_REF,
                Path::new("notes/a.md"),
                format!("# revision {n}").as_bytes(),
                "update a note",
                &author(),
            )
            .unwrap();
        }

        assert_eq!(
            repo.head().unwrap().target().unwrap(),
            head_before,
            "the checked-out branch must not advance"
        );
        assert!(
            repo.head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .tree()
                .unwrap()
                .get_path(Path::new("notes/a.md"))
                .is_err(),
            "notes must not enter the tree the branch points at"
        );
    }

    #[test]
    fn negative_control_targeting_the_checked_out_branch_does_move_head() {
        // Establishes that the property above is not vacuous: the same call aimed at the
        // checked-out branch advances it.
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("app.txt"),
            b"their work",
            "their work",
            &author(),
        )
        .unwrap();
        repo.set_head("refs/heads/main").unwrap();
        let head_before = repo.head().unwrap().target().unwrap();

        commit_file(
            &repo,
            "refs/heads/main",
            Path::new("notes/a.md"),
            b"# heading",
            "add a note",
            &author(),
        )
        .unwrap();

        assert_ne!(repo.head().unwrap().target().unwrap(), head_before);
    }

    #[test]
    fn a_clean_repository_reports_no_operation_in_progress() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        assert_eq!(operation_in_progress(&repo), None);
    }

    #[test]
    fn a_repository_mid_operation_reports_it() {
        // Each marker is what the library itself looks for when classifying repository state,
        // so writing one reproduces the condition without running the operation.
        let cases: [(&str, bool); 4] = [
            ("MERGE_HEAD", false),
            ("BISECT_LOG", false),
            ("CHERRY_PICK_HEAD", false),
            ("rebase-merge", true),
        ];

        for (marker, is_dir) in cases {
            let dir = TempDir::new().unwrap();
            let repo = Repository::init(dir.path()).unwrap();
            let marker_path = repo.path().join(marker);
            if is_dir {
                std::fs::create_dir_all(&marker_path).unwrap();
            } else {
                std::fs::write(&marker_path, "0\n").unwrap();
            }

            assert!(
                operation_in_progress(&repo).is_some(),
                "{marker} must be reported as an operation in progress"
            );
        }
    }

    #[test]
    fn preparing_a_plain_folder_creates_a_repository_there() {
        let dir = TempDir::new().unwrap();
        let vault = dir.path().join("notes");
        std::fs::create_dir(&vault).unwrap();

        let prepared = prepare(&vault).unwrap();
        assert!(vault.join(".git").exists());
        assert_eq!(prepared.path_in_repo(Path::new("a.md")), PathBuf::from("a.md"));

        // Preparing again opens what is already there rather than starting over.
        let again = prepare(&vault).unwrap();
        assert_eq!(again.repo().path(), prepared.repo().path());
    }

    #[test]
    fn several_paths_become_one_revision() {
        // A change that spans files has to land as one revision: committing them separately
        // publishes states in which the links between them point at things that are not
        // there yet.
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let entries = vec![
            (PathBuf::from("a.md"), b"# a links to b".to_vec()),
            (PathBuf::from("sub/b.md"), b"# b".to_vec()),
        ];

        commit_paths(
            &repo,
            NOTES_REF,
            &entries,
            "move a section out",
            &author(),
            &author(),
        )
        .unwrap();

        let head = repo
            .find_reference(NOTES_REF)
            .unwrap()
            .peel_to_commit()
            .unwrap();
        assert_eq!(head.parent_count(), 0, "one revision, not two");
        let tree = head.tree().unwrap();
        assert!(tree.get_path(Path::new("a.md")).is_ok());
        assert!(tree.get_path(&Path::new("sub").join("b.md")).is_ok());
    }

    #[test]
    fn committing_nothing_is_refused() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        assert!(commit_paths(&repo, NOTES_REF, &[], "empty", &author(), &author()).is_err());
        assert!(repo.find_reference(NOTES_REF).is_err(), "no reference is created");
    }

    #[test]
    fn the_author_is_the_repository_owner_and_the_committer_is_the_application() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Their Name").unwrap();
        config.set_str("user.email", "them@example.invalid").unwrap();

        let (author, committer) = commit_identities(&repo).unwrap();
        assert_eq!(author.name().unwrap(), "Their Name");
        assert_eq!(author.email().unwrap(), "them@example.invalid");
        assert_ne!(
            committer.email().unwrap(),
            author.email().unwrap(),
            "the application records itself as the committer"
        );
    }

    #[test]
    fn a_missing_identity_falls_back_to_the_application() {
        // Configuring an identity is a step many repositories never take, and asking for one
        // mid-save would interrupt writing a note over something the note does not need.
        //
        // The absence is injected rather than arranged on disk: clearing the repository's own
        // configuration would still leave a machine-wide identity in place, so a test built
        // that way passes without ever reaching the fallback.
        let unconfigured = Err(git2::Error::from_str("no identity configured"));
        let (author, committer) = identities_from(unconfigured).unwrap();

        assert_eq!(author.email().unwrap(), committer.email().unwrap());
        assert_eq!(author.name().unwrap(), APP_COMMITTER_NAME);

        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        commit_paths(
            &repo,
            NOTES_REF,
            &[(PathBuf::from("a.md"), b"# heading".to_vec())],
            "add a note",
            &author,
            &committer,
        )
        .expect("committing must not depend on a configured identity");
    }

    #[test]
    fn preparing_a_repository_without_a_working_tree_is_refused() {
        let dir = TempDir::new().unwrap();
        Repository::init_bare(dir.path()).unwrap();
        assert!(prepare(dir.path()).is_err());
    }

    #[test]
    fn a_fresh_vault_leaves_the_default_branch_unborn() {
        // Records the shape a brand-new vault actually has. Notes live on their own reference,
        // so the branch `init` set up never receives a commit: `HEAD` stays unborn and branch
        // listings are empty. A tool that shows only branches shows nothing here, while
        // `--all` and reference enumeration show the notes.
        let dir = TempDir::new().unwrap();
        let vault = dir.path().join("notes");
        std::fs::create_dir(&vault).unwrap();

        let prepared = prepare(&vault).unwrap();
        commit_file(
            prepared.repo(),
            NOTES_REF,
            Path::new("a.md"),
            b"# heading",
            "add a note",
            &author(),
        )
        .unwrap();

        assert!(
            prepared.repo().head().is_err(),
            "the default branch never receives a commit"
        );
        assert_eq!(prepared.repo().branches(None).unwrap().count(), 0);
        assert!(prepared.repo().find_reference(NOTES_REF).is_ok());
    }

    #[test]
    fn preparing_a_folder_inside_a_repository_does_not_nest_one() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        let vault = dir.path().join("docs").join("notes");
        std::fs::create_dir_all(&vault).unwrap();

        let prepared = prepare(&vault).unwrap();

        assert!(
            !vault.join(".git").exists(),
            "a repository inside a repository is what this avoids"
        );
        assert_eq!(
            prepared.path_in_repo(Path::new("a.md")),
            Path::new("docs").join("notes").join("a.md"),
            "paths are written where the folder actually sits"
        );

        commit_file(
            prepared.repo(),
            NOTES_REF,
            &prepared.path_in_repo(Path::new("a.md")),
            b"# heading",
            "add a note",
            &author(),
        )
        .unwrap();

        let tree = prepared
            .repo()
            .find_reference(NOTES_REF)
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .tree()
            .unwrap();
        assert!(tree
            .get_path(&Path::new("docs").join("notes").join("a.md"))
            .is_ok());
    }

    #[test]
    fn staging_everything_in_the_outer_repository_stays_unaffected() {
        let dir = TempDir::new().unwrap();
        let outer = Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("app.txt"), "their work").unwrap();
        let vault = dir.path().join("docs");
        std::fs::create_dir_all(&vault).unwrap();

        prepare(&vault).unwrap();
        std::fs::write(vault.join("a.md"), "# heading").unwrap();

        let mut index = outer.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("staging everything must keep working");

        // Nothing was turned into a repository link; the note is an ordinary tracked file.
        let entry = index.get_path(&Path::new("docs").join("a.md"), 0).unwrap();
        assert_eq!(entry.mode, 0o100644, "the note must be a plain blob entry");
    }

    #[test]
    fn negative_control_a_nested_repository_breaks_staging_for_the_whole_repository() {
        // Establishes that the avoided arrangement causes real harm, so the test above is not
        // guarding against nothing. Staging everything is an everyday command, and with a
        // repository nested inside one it errors out partway: the caller gets a failure, the
        // nested folder contributes nothing, and whatever had already been walked stays
        // staged — a partial result nobody asked for.
        let dir = TempDir::new().unwrap();
        let outer = Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("app.txt"), "their work").unwrap();
        let vault = dir.path().join("docs");
        std::fs::create_dir_all(&vault).unwrap();

        let inner = Repository::init(&vault).unwrap();
        std::fs::write(vault.join("a.md"), "# heading").unwrap();
        commit_file(
            &inner,
            "refs/heads/main",
            Path::new("a.md"),
            b"# heading",
            "seed",
            &author(),
        )
        .unwrap();
        inner.set_head("refs/heads/main").unwrap();

        let mut index = outer.index().unwrap();
        let result = index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None);

        assert!(result.is_err(), "staging everything is expected to fail");
        assert!(
            index.get_path(&Path::new("docs").join("a.md"), 0).is_none(),
            "the nested folder's file cannot be staged"
        );
        assert!(
            index.get_path(Path::new("docs"), 0).is_none(),
            "and the folder itself contributes no entry"
        );
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

    /// Records `rel` with the given body and returns the revision it produced.
    fn record(repo: &Repository, rel: &Path, body: &str) -> Oid {
        commit_file(repo, NOTES_REF, rel, body.as_bytes(), "recorded", &author()).unwrap()
    }

    #[test]
    fn a_nested_path_is_recognised_as_recorded() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        // Built the way the rest of the application builds paths, so it carries the platform's
        // separator rather than the one trees are written with.
        let nested = Path::new("docs").join("guide.md");

        record(&repo, &nested, "body");

        assert!(
            is_recorded(&repo, NOTES_REF, &nested),
            "a path one level down must be found"
        );
        assert!(
            !is_recorded(&repo, NOTES_REF, Path::new("docs/missing.md")),
            "and one that was never recorded must not be"
        );
    }

    #[test]
    fn history_lists_only_the_revisions_that_changed_that_path() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let a = Path::new("a.md");
        let b = Path::new("notes").join("b.md");

        record(&repo, a, "one");
        record(&repo, a, "two");
        record(&repo, &b, "other");
        record(&repo, a, "three");

        let for_a = history(&repo, NOTES_REF, a).unwrap();
        assert_eq!(for_a.len(), 3, "the other note's revisions must not appear");
        let for_b = history(&repo, NOTES_REF, &b).unwrap();
        assert_eq!(for_b.len(), 1);

        assert!(
            for_a[0].seconds >= for_a[for_a.len() - 1].seconds,
            "newest first"
        );
        assert_eq!(for_a[0].author, "Test");
    }

    #[test]
    fn history_of_a_reference_that_does_not_exist_is_empty() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        assert!(history(&repo, NOTES_REF, Path::new("a.md"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn negative_control_the_checked_out_branch_holds_none_of_this() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let a = Path::new("a.md");
        record(&repo, a, "one");
        record(&repo, a, "two");

        // Reading the branch a person checks out finds nothing, which is what makes the note
        // history invisible to their own work — and proves the reads above are aimed elsewhere.
        for candidate in ["refs/heads/main", "refs/heads/master"] {
            assert!(history(&repo, candidate, a).unwrap().is_empty());
            assert!(tip_paths(&repo, candidate).unwrap().is_empty());
        }
    }

    #[test]
    fn recording_the_same_content_again_adds_nothing() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let a = Path::new("a.md");

        let first = record(&repo, a, "same");
        let second = record(&repo, a, "same");

        assert_eq!(first, second, "an unchanged state is not a new revision");
        assert_eq!(history(&repo, NOTES_REF, a).unwrap().len(), 1);
    }

    #[test]
    fn an_earlier_state_can_be_read_back_without_touching_the_disk() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let nested = Path::new("docs").join("guide.md");

        record(&repo, &nested, "first");
        record(&repo, &nested, "second");

        let entries = history(&repo, NOTES_REF, &nested).unwrap();
        let earliest = &entries[entries.len() - 1];
        assert_eq!(
            content_at(&repo, &earliest.id, &nested).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            content_at_tip(&repo, NOTES_REF, &nested).unwrap().unwrap(),
            b"second"
        );
        assert!(
            content_at(&repo, &earliest.id, Path::new("absent.md"))
                .unwrap()
                .is_none()
        );
        assert!(
            !dir.path().join("docs").exists(),
            "reading history must not materialise anything"
        );
    }

    #[test]
    fn every_recorded_path_is_listed_with_when_it_last_changed() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let a = Path::new("a.md");
        let b = Path::new("notes").join("b.md");

        record(&repo, a, "one");
        record(&repo, &b, "other");
        record(&repo, a, "two");

        let mut paths = tip_paths(&repo, NOTES_REF).unwrap();
        paths.sort();
        assert_eq!(paths, vec!["a.md".to_string(), "notes/b.md".to_string()]);

        let times = last_changed(&repo, NOTES_REF).unwrap();
        assert!(times.contains_key("a.md") && times.contains_key("notes/b.md"));
        assert!(
            times["a.md"] >= times["notes/b.md"],
            "the later change must not be reported as older"
        );
    }
}
