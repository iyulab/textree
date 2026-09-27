//! Exchanging recorded notes with a remote.
//!
//! Only the app's own references travel: everything under `refs/textree/`. What arrives is kept
//! under [`TRACKING`] first — outside `refs/textree/`, so it is never sent back — and then weighed
//! against what this machine recorded before anything moves:
//!
//! - one side is simply ahead: the other catches up;
//! - the two sides changed **different** notes: they are joined path by path. No note's text is
//!   merged, and both sides' revisions stay in the history, so nothing is lost;
//! - the two sides changed the **same** note (or one deleted what the other changed): nothing
//!   moves. The incoming state stays where it arrived until a person decides.
//!
//! Working out what would happen ([`plan`]) is separate from making it happen, and reads only:
//! a join is prepared as an unreferenced revision, which is harmless if never used. Moving a
//! reference has to happen together with writing the notes it changes to disk — a reference that
//! ran ahead of the files would let the next recorded note quietly undo what arrived.

use std::collections::BTreeSet;
use std::path::Path;

use git2::{Diff, DiffOptions, ErrorCode, Index, IndexEntry, IndexTime, Oid, Repository, Tree};

use crate::git_engine::{commit_identities, VaultRepo, NOTES_REF, SNAPSHOT_REF};
use crate::git_transport::{with_credentials, Credentials};

/// Where the remote's references are kept once fetched: `refs/textree/notes` arrives as
/// `refs/textree-remote/notes`.
pub const TRACKING: &str = "refs/textree-remote/";

const OWN: &str = "refs/textree/";

/// The message of a revision that joins two sides which changed different notes.
pub const JOIN_MESSAGE: &str = "Joined changes from another device";

fn tracking_of(local: &str) -> String {
    format!("{TRACKING}{}", local.strip_prefix(OWN).unwrap_or(local))
}

/// How sending went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// The remote now holds what this machine recorded.
    Done,
    /// Nothing has been recorded here yet.
    NothingToSend,
    /// The remote holds something this machine has not taken in. Fetch, take it in, send again.
    Behind,
}

/// Sends every reference under `refs/textree/`. Never forces: a remote that moved on since this
/// machine last took it in refuses, and that is reported as [`Sent::Behind`].
pub fn push(
    repo: &Repository,
    url: &str,
    credentials: Option<Credentials>,
) -> Result<Sent, git2::Error> {
    crate::git_transport::install();
    let mut names = Vec::new();
    for reference in repo.references_glob(&format!("{OWN}*"))? {
        if let Ok(name) = reference?.name() {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Ok(Sent::NothingToSend);
    }
    // libgit2 resolves each source by name, so a glob cannot be pushed as one spec.
    let specs: Vec<String> = names.iter().map(|n| format!("{n}:{n}")).collect();
    let specs: Vec<&str> = specs.iter().map(String::as_str).collect();

    let mut refused = false;
    let outcome = {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.push_update_reference(|_, status| {
            if status.is_some() {
                refused = true;
            }
            Ok(())
        });
        let mut options = git2::PushOptions::new();
        options.remote_callbacks(callbacks);
        with_credentials(credentials, || {
            repo.remote_anonymous(url)?.push(&specs, Some(&mut options))
        })
    };
    match outcome {
        Err(e) if e.code() == ErrorCode::NotFastForward => Ok(Sent::Behind),
        Err(e) => Err(e),
        Ok(()) if refused => Ok(Sent::Behind),
        Ok(()) => Ok(Sent::Done),
    }
}

/// Brings the remote's references in under [`TRACKING`]. Moves nothing this machine recorded.
pub fn fetch(
    repo: &Repository,
    url: &str,
    credentials: Option<Credentials>,
) -> Result<(), git2::Error> {
    crate::git_transport::install();
    // Forced into the tracking names only: they mirror the remote, whatever it did.
    let spec = format!("+{OWN}*:{TRACKING}*");
    with_credentials(credentials, || {
        repo.remote_anonymous(url)?
            .fetch(&[spec.as_str()], None, None)
    })
}

/// What taking in one reference would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing arrived, or this machine already has it.
    Nothing,
    /// This machine had recorded nothing on this reference; it takes the remote's as it is.
    Adopt { to: Oid },
    /// The remote is simply ahead.
    FastForward { from: Oid, to: Oid },
    /// The two sides changed different notes; `commit` holds both. `incoming` are the paths the
    /// remote's side changed, which this machine's files have to take.
    Join {
        from: Oid,
        commit: Oid,
        incoming: Vec<String>,
    },
    /// Both sides changed these paths. Nothing moves until a person decides.
    Held { overlap: Vec<String> },
}

/// What taking in everything that arrived would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub notes: Step,
    pub snapshots: Step,
    /// Notes the remote changed whose file here holds edits nobody recorded yet. Writing the
    /// remote's version would overwrite them, so these must not be written.
    pub unrecorded_here: Vec<String>,
}

/// Works out what taking in the fetched references would do. Reads only; see the module notes.
pub fn plan(vault: &VaultRepo, root: &Path) -> Result<Plan, git2::Error> {
    let repo = vault.repo();
    // Two notes changed on the two sides are two notes: the notes reference joins only when the
    // changed paths do not meet.
    let notes = plan_ref(repo, NOTES_REF, Overlap::Hold)?;
    // What was kept of deleted notes is only ever added to. The same path kept on both sides is
    // kept on both — this side's copy in the joined tree, the other's in its own revision.
    let snapshots = plan_ref(repo, SNAPSHOT_REF, Overlap::KeepOurs)?;
    let unrecorded_here = unrecorded_among(vault, root, &notes)?;
    Ok(Plan {
        notes,
        snapshots,
        unrecorded_here,
    })
}

#[derive(Clone, Copy)]
enum Overlap {
    Hold,
    KeepOurs,
}

fn plan_ref(repo: &Repository, local: &str, overlap: Overlap) -> Result<Step, git2::Error> {
    let Some(theirs) = id_of(repo, &tracking_of(local)) else {
        return Ok(Step::Nothing);
    };
    let Some(ours) = id_of(repo, local) else {
        return Ok(Step::Adopt { to: theirs });
    };
    if ours == theirs || repo.graph_descendant_of(ours, theirs)? {
        return Ok(Step::Nothing);
    }
    if repo.graph_descendant_of(theirs, ours)? {
        return Ok(Step::FastForward {
            from: ours,
            to: theirs,
        });
    }

    let our_tree = repo.find_commit(ours)?.tree()?;
    let their_tree = repo.find_commit(theirs)?.tree()?;
    // Two histories that never shared a revision still share the empty folder they started from.
    let base_tree = match repo.merge_base(ours, theirs) {
        Ok(base) => Some(repo.find_commit(base)?.tree()?),
        Err(e) if e.code() == ErrorCode::NotFound => None,
        Err(e) => return Err(e),
    };
    let ours_changed = changed(repo, base_tree.as_ref(), &our_tree)?;
    let theirs_changed = changed(repo, base_tree.as_ref(), &their_tree)?;
    let met: Vec<String> = ours_changed
        .intersection(&theirs_changed)
        .cloned()
        .collect();
    let incoming: Vec<String> = match overlap {
        Overlap::Hold if !met.is_empty() => return Ok(Step::Held { overlap: met }),
        _ => theirs_changed.difference(&ours_changed).cloned().collect(),
    };

    let tree = joined_tree(repo, &our_tree, &their_tree, &incoming)?;
    let (author, committer) = commit_identities(repo)?;
    let parents = [&repo.find_commit(ours)?, &repo.find_commit(theirs)?];
    let commit = repo.commit(
        None,
        &author,
        &committer,
        JOIN_MESSAGE,
        &repo.find_tree(tree)?,
        &parents,
    )?;
    Ok(Step::Join {
        from: ours,
        commit,
        incoming,
    })
}

fn id_of(repo: &Repository, name: &str) -> Option<Oid> {
    repo.find_reference(name).ok().and_then(|r| r.target())
}

/// Every path whose content differs between `from` (the empty tree when `None`) and `to`.
/// Without rename detection on purpose: a rename is its old path removed and its new one added,
/// so an edit to the old path on the other side counts as meeting it.
fn changed(
    repo: &Repository,
    from: Option<&Tree<'_>>,
    to: &Tree<'_>,
) -> Result<BTreeSet<String>, git2::Error> {
    let mut options = DiffOptions::new();
    options.ignore_submodules(true);
    let diff: Diff<'_> = repo.diff_tree_to_tree(from, Some(to), Some(&mut options))?;
    let mut out = BTreeSet::new();
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(path) = file.path().and_then(Path::to_str) {
                out.insert(path.replace('\\', "/"));
            }
        }
    }
    Ok(out)
}

/// This side's tree with the other side's state of each `incoming` path laid over it.
fn joined_tree(
    repo: &Repository,
    ours: &Tree<'_>,
    theirs: &Tree<'_>,
    incoming: &[String],
) -> Result<Oid, git2::Error> {
    let mut index = Index::new()?;
    index.read_tree(ours)?;
    for path in incoming {
        match theirs.get_path(Path::new(path)) {
            Ok(entry) => {
                let zero = IndexTime::new(0, 0);
                index.add(&IndexEntry {
                    ctime: zero,
                    mtime: zero,
                    dev: 0,
                    ino: 0,
                    mode: entry.filemode() as u32,
                    uid: 0,
                    gid: 0,
                    file_size: 0,
                    id: entry.id(),
                    flags: 0,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                })?;
            }
            Err(e) if e.code() == ErrorCode::NotFound => index.remove_path(Path::new(path))?,
            Err(e) => return Err(e),
        }
    }
    index.write_tree_to(repo)
}

/// The notes a step would write to disk whose file here differs from what this machine last
/// recorded — an edit that was saved but not yet added as a version.
fn unrecorded_among(
    vault: &VaultRepo,
    root: &Path,
    step: &Step,
) -> Result<Vec<String>, git2::Error> {
    let repo = vault.repo();
    let (ours, target) = match step {
        Step::Adopt { to } => (None, *to),
        Step::FastForward { from, to } => (Some(*from), *to),
        Step::Join { from, commit, .. } => (Some(*from), *commit),
        Step::Nothing | Step::Held { .. } => return Ok(Vec::new()),
    };
    let our_tree = match ours {
        Some(id) => Some(repo.find_commit(id)?.tree()?),
        None => None,
    };
    let target_tree = repo.find_commit(target)?.tree()?;
    let mut out = Vec::new();
    for path in changed(repo, our_tree.as_ref(), &target_tree)? {
        let Some(in_vault) = vault.path_in_vault(Path::new(&path)) else {
            continue;
        };
        let on_disk = std::fs::read(root.join(&in_vault)).ok();
        let recorded = match &our_tree {
            Some(tree) => match tree.get_path(Path::new(&path)) {
                Ok(entry) => Some(repo.find_blob(entry.id())?.content().to_vec()),
                Err(e) if e.code() == ErrorCode::NotFound => None,
                Err(e) => return Err(e),
            },
            None => None,
        };
        if on_disk != recorded {
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_engine::{commit_paths, WhenUnchanged};
    use std::path::PathBuf;

    struct Machine {
        root: PathBuf,
        vault: VaultRepo,
    }

    impl Machine {
        fn new(root: PathBuf) -> Machine {
            std::fs::create_dir_all(&root).unwrap();
            let vault = crate::git_engine::prepare(&root).unwrap();
            Machine { root, vault }
        }

        fn repo(&self) -> &Repository {
            self.vault.repo()
        }

        /// Saves `text` to the note's file and records it, as adding a version does.
        fn record(&self, rel: &str, text: &str) {
            self.save(rel, text);
            let (author, committer) = commit_identities(self.repo()).unwrap();
            commit_paths(
                self.repo(),
                NOTES_REF,
                &[(PathBuf::from(rel), text.as_bytes().to_vec())],
                "version",
                &author,
                &committer,
                WhenUnchanged::Skip,
            )
            .unwrap();
        }

        fn save(&self, rel: &str, text: &str) {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn url_of(remote: &Path) -> String {
            remote.to_string_lossy().into_owned()
        }

        fn send(&self, remote: &Path) -> Sent {
            push(self.repo(), &Machine::url_of(remote), None).unwrap()
        }

        fn take(&self, remote: &Path) -> Plan {
            fetch(self.repo(), &Machine::url_of(remote), None).unwrap();
            plan(&self.vault, &self.root).unwrap()
        }

        fn tip(&self) -> Oid {
            id_of(self.repo(), NOTES_REF).unwrap()
        }

        /// Moves the notes reference as taking a step in would (the disk half is not built yet).
        fn apply(&self, step: &Step) {
            let (from, to) = match step {
                Step::Adopt { to } => (None, *to),
                Step::FastForward { from, to } => (Some(*from), *to),
                Step::Join { from, commit, .. } => (Some(*from), *commit),
                _ => return,
            };
            match from {
                Some(from) => self
                    .repo()
                    .reference_matching(NOTES_REF, to, true, from, "take in")
                    .unwrap(),
                None => self
                    .repo()
                    .reference(NOTES_REF, to, false, "take in")
                    .unwrap(),
            };
        }
    }

    fn setup() -> (tempfile::TempDir, PathBuf, Machine, Machine) {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        Repository::init_bare(&remote).unwrap();
        let a = Machine::new(tmp.path().join("a"));
        let b = Machine::new(tmp.path().join("b"));
        (tmp, remote, a, b)
    }

    #[test]
    fn a_first_machine_sends_and_a_second_adopts() {
        let (_tmp, remote, a, b) = setup();
        assert_eq!(
            push(a.repo(), &Machine::url_of(&remote), None).unwrap(),
            Sent::NothingToSend
        );
        a.record("manual.md", "# Manual\n");
        assert_eq!(a.send(&remote), Sent::Done);

        let plan = b.take(&remote);
        assert_eq!(plan.notes, Step::Adopt { to: a.tip() });
        assert!(plan.unrecorded_here.is_empty());
        // Arriving is not taking in: this machine's own reference has not moved.
        assert!(id_of(b.repo(), NOTES_REF).is_none());
    }

    #[test]
    fn a_remote_that_is_simply_ahead_is_caught_up_with() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);
        a.record("manual.md", "# Manual\n\nMore.\n");
        a.send(&remote);

        let plan = b.take(&remote);
        assert!(matches!(plan.notes, Step::FastForward { to, .. } if to == a.tip()));
        // Nothing moves on this side's own references by planning.
        assert_ne!(b.tip(), a.tip());
    }

    #[test]
    fn different_notes_changed_on_two_machines_are_joined_without_losing_either() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);

        a.record("release-notes.md", "# 1.0\n");
        a.send(&remote);
        b.record("faq.md", "# FAQ\n");
        // b is behind: its send is refused rather than dropping a's release notes.
        assert_eq!(b.send(&remote), Sent::Behind);

        let plan = b.take(&remote);
        let Step::Join {
            commit, incoming, ..
        } = &plan.notes
        else {
            panic!("expected a join, got {:?}", plan.notes);
        };
        assert_eq!(incoming, &vec!["release-notes.md".to_string()]);
        let joined = b.repo().find_commit(*commit).unwrap();
        assert_eq!(joined.parent_count(), 2);
        let tree = joined.tree().unwrap();
        for path in ["manual.md", "release-notes.md", "faq.md"] {
            assert!(
                tree.get_path(Path::new(path)).is_ok(),
                "{path} missing from the join"
            );
        }
        // The file the remote brought has no edits here, so it may be written.
        assert!(plan.unrecorded_here.is_empty());

        b.apply(&plan.notes);
        assert_eq!(b.send(&remote), Sent::Done);
    }

    #[test]
    fn the_same_note_changed_on_two_machines_is_held() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);

        a.record("manual.md", "# Manual\n\nFrom a.\n");
        a.send(&remote);
        b.record("manual.md", "# Manual\n\nFrom b.\n");
        let before = b.tip();

        let plan = b.take(&remote);
        assert_eq!(
            plan.notes,
            Step::Held {
                overlap: vec!["manual.md".to_string()]
            }
        );
        assert_eq!(b.tip(), before);
        assert!(
            plan.unrecorded_here.is_empty(),
            "nothing is written while held"
        );
    }

    #[test]
    fn a_note_deleted_on_one_machine_and_changed_on_the_other_is_held() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.record("old.md", "# Old\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);

        // a deletes old.md: the recorded tree loses it.
        {
            let repo = a.repo();
            let tip = repo.find_commit(a.tip()).unwrap();
            let mut index = Index::new().unwrap();
            index.read_tree(&tip.tree().unwrap()).unwrap();
            index.remove_path(Path::new("old.md")).unwrap();
            let tree = repo.find_tree(index.write_tree_to(repo).unwrap()).unwrap();
            let (author, committer) = commit_identities(repo).unwrap();
            repo.commit(
                Some(NOTES_REF),
                &author,
                &committer,
                "delete",
                &tree,
                &[&tip],
            )
            .unwrap();
        }
        a.send(&remote);
        b.record("old.md", "# Old, kept\n");

        let plan = b.take(&remote);
        assert_eq!(
            plan.notes,
            Step::Held {
                overlap: vec!["old.md".to_string()]
            }
        );
    }

    #[test]
    fn an_incoming_note_with_unrecorded_edits_here_is_not_to_be_written() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);
        b.save("manual.md", "# Manual\n\nTyped here, not yet a version.\n");

        a.record("manual.md", "# Manual\n\nFrom a.\n");
        a.send(&remote);

        let plan = b.take(&remote);
        assert!(matches!(plan.notes, Step::FastForward { .. }));
        assert_eq!(plan.unrecorded_here, vec!["manual.md".to_string()]);
    }

    #[test]
    fn a_join_is_not_a_version_of_the_notes_it_carried() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.apply(&b.take(&remote).notes);
        a.record("release-notes.md", "# 1.0\n");
        a.send(&remote);
        b.record("faq.md", "# FAQ\n");
        let plan = b.take(&remote);
        b.apply(&plan.notes);

        for (path, expected) in [("release-notes.md", 1), ("faq.md", 1), ("manual.md", 1)] {
            let versions =
                crate::git_engine::history(b.repo(), NOTES_REF, Path::new(path)).unwrap();
            assert_eq!(versions.len(), expected, "{path}: {versions:?}");
            assert!(
                versions.iter().all(|v| v.message != JOIN_MESSAGE),
                "{path}: {versions:?}"
            );
        }
    }

    fn keep(machine: &Machine, rel: &str, text: &str) {
        let (author, committer) = commit_identities(machine.repo()).unwrap();
        commit_paths(
            machine.repo(),
            SNAPSHOT_REF,
            &[(PathBuf::from(rel), text.as_bytes().to_vec())],
            "kept",
            &author,
            &committer,
            WhenUnchanged::Skip,
        )
        .unwrap();
    }

    #[test]
    fn what_was_kept_of_deleted_notes_is_joined_even_when_both_sides_kept_the_same_path() {
        let (_tmp, remote, a, b) = setup();
        keep(
            &a,
            "draft.md",
            "# From a
",
        );
        keep(
            &a,
            "only-a.md",
            "# A
",
        );
        a.send(&remote);
        keep(
            &b,
            "draft.md",
            "# From b
",
        );
        keep(
            &b,
            "only-b.md",
            "# B
",
        );

        let plan = b.take(&remote);
        let Step::Join { commit, .. } = plan.snapshots else {
            panic!("expected a join, got {:?}", plan.snapshots);
        };
        let joined = b.repo().find_commit(commit).unwrap();
        let tree = joined.tree().unwrap();
        for path in ["draft.md", "only-a.md", "only-b.md"] {
            assert!(tree.get_path(Path::new(path)).is_ok(), "{path} missing");
        }
        // This side's copy of the shared path is in the tree; the other side's stays reachable
        // through the second parent.
        let ours = tree.get_path(Path::new("draft.md")).unwrap().id();
        assert_eq!(
            b.repo().find_blob(ours).unwrap().content(),
            b"# From b
"
        );
        let theirs = joined.parent(1).unwrap().tree().unwrap();
        let theirs = theirs.get_path(Path::new("draft.md")).unwrap().id();
        assert_eq!(
            b.repo().find_blob(theirs).unwrap().content(),
            b"# From a
"
        );
    }

    #[test]
    fn what_arrives_is_never_sent_back() {
        let (_tmp, remote, a, b) = setup();
        a.record("manual.md", "# Manual\n");
        a.send(&remote);
        b.take(&remote);
        b.record("faq.md", "# FAQ\n");
        // b has not taken a's notes in; its send must not carry the tracking copy along.
        assert_eq!(b.send(&remote), Sent::Behind);
        let bare = Repository::open_bare(&remote).unwrap();
        let names: Vec<String> = bare
            .references()
            .unwrap()
            .filter_map(|r| r.ok().and_then(|r| r.name().ok().map(str::to_string)))
            .collect();
        assert!(names.iter().all(|n| !n.starts_with(TRACKING)), "{names:?}");
    }
}
