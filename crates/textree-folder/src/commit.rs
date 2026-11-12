//! Committing a change set to a folder: all of it or none of it.
//!
//! Every step that changes the folder is recorded in a journal; if a step fails, the journal is
//! played backwards and the folder is as it was. Deleted files are first set aside under a
//! hidden name and only removed for good once everything else has gone through.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use textree::{validate_paths, ChangeSet, CommitError, ReadPort};

use crate::FolderStore;

enum Undo {
    /// Rename `from` back to `to`.
    Rename { from: PathBuf, to: PathBuf },
    /// Put the old bytes of a file back.
    Restore { path: PathBuf, bytes: Vec<u8> },
    /// Remove a folder that the commit created.
    RemoveDir(PathBuf),
    /// Remove a file that the commit created.
    RemoveFile(PathBuf),
}

struct Run<'a> {
    root: &'a Path,
    fail_at: Option<usize>,
    step: usize,
    undo: Vec<Undo>,
    set_aside: Vec<PathBuf>,
    unique: usize,
}

impl Run<'_> {
    /// Counts a mutating step; the test hook makes the chosen one fail.
    fn step(&mut self) -> io::Result<()> {
        self.step += 1;
        if self.fail_at == Some(self.step) {
            return Err(io::Error::other("injected failure"));
        }
        Ok(())
    }

    fn temp_name(&mut self, near: &Path, label: &str) -> PathBuf {
        self.unique += 1;
        near.with_file_name(format!(
            ".textree-{label}-{}-{}",
            std::process::id(),
            self.unique
        ))
    }

    fn make_dirs(&mut self, dir: &Path) -> io::Result<()> {
        let mut missing = Vec::new();
        let mut cur = dir;
        while cur != self.root && !cur.exists() {
            missing.push(cur.to_path_buf());
            match cur.parent() {
                Some(p) => cur = p,
                None => break,
            }
        }
        for d in missing.into_iter().rev() {
            self.step()?;
            fs::create_dir(&d)?;
            self.undo.push(Undo::RemoveDir(d));
        }
        Ok(())
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.step()?;
        let same_name_but_case = textree::path::fold(&from.to_string_lossy())
            == textree::path::fold(&to.to_string_lossy());
        if same_name_but_case {
            // On a file system that ignores case a direct rename may be a no-op; go through a
            // temporary name.
            let tmp = self.temp_name(from, "case");
            fs::rename(from, &tmp)?;
            if let Err(e) = fs::rename(&tmp, to) {
                let _ = fs::rename(&tmp, from);
                return Err(e);
            }
        } else {
            fs::rename(from, to)?;
        }
        self.undo.push(Undo::Rename {
            from: to.to_path_buf(),
            to: from.to_path_buf(),
        });
        Ok(())
    }

    fn write(&mut self, path: &Path, content: &[u8]) -> io::Result<()> {
        let old = fs::read(path)?;
        let tmp = self.temp_name(path, "tmp");
        self.step()?;
        let written = fs::write(&tmp, content).map(|_| {
            if let Ok(meta) = fs::metadata(path) {
                let _ = fs::set_permissions(&tmp, meta.permissions());
            }
        });
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        if let Err(e) = self.step().and_then(|_| fs::rename(&tmp, path)) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        self.undo.push(Undo::Restore {
            path: path.to_path_buf(),
            bytes: old,
        });
        Ok(())
    }

    /// Creates a file that does not exist yet; never replaces one.
    fn create(&mut self, path: &Path, content: &[u8]) -> io::Result<()> {
        use std::io::Write as _;
        self.step()?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        self.undo.push(Undo::RemoveFile(path.to_path_buf()));
        self.step()?;
        file.write_all(content)?;
        file.sync_all()?;
        Ok(())
    }

    fn set_aside_for_delete(&mut self, path: &Path) -> io::Result<()> {
        let hidden = self.temp_name(path, "trash");
        self.step()?;
        fs::rename(path, &hidden)?;
        self.undo.push(Undo::Rename {
            from: hidden.clone(),
            to: path.to_path_buf(),
        });
        self.set_aside.push(hidden);
        Ok(())
    }

    /// Plays the journal backwards. Returns what could not be put back.
    fn roll_back(&mut self) -> Vec<String> {
        let mut problems = Vec::new();
        while let Some(u) = self.undo.pop() {
            let result = match &u {
                Undo::Rename { from, to } => fs::rename(from, to),
                Undo::Restore { path, bytes } => fs::write(path, bytes),
                Undo::RemoveDir(d) => fs::remove_dir(d),
                Undo::RemoveFile(f) => fs::remove_file(f),
            };
            if let Err(e) = result {
                problems.push(match &u {
                    Undo::Rename { from, to } => {
                        format!("{} -> {}: {e}", from.display(), to.display())
                    }
                    Undo::Restore { path, .. } => format!("{}: {e}", path.display()),
                    Undo::RemoveDir(d) => format!("{}: {e}", d.display()),
                    Undo::RemoveFile(f) => format!("{}: {e}", f.display()),
                });
            }
        }
        problems
    }
}

fn under(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for piece in rel.split('/') {
        p.push(piece);
    }
    p
}

/// A check of the folder after the changes were made, before they are final.
pub(crate) type Verify<'a> = &'a dyn Fn(&FolderStore) -> Result<(), String>;

/// Whether a path goes through a symbolic link on its way down (the last piece may be one).
fn goes_through_symlink(root: &Path, rel: &str) -> bool {
    let pieces: Vec<&str> = rel.split('/').collect();
    let mut cur = root.to_path_buf();
    for piece in &pieces[..pieces.len().saturating_sub(1)] {
        cur.push(piece);
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return true;
            }
        }
    }
    false
}

/// What can be told about a change set without changing anything: it is well formed, every
/// path stays inside the root and none goes through a symbolic link (which could lead out).
pub(crate) fn preflight(store: &FolderStore, changes: &ChangeSet) -> Result<(), CommitError> {
    if changes.version != 1 && changes.version != 2 {
        return Err(CommitError::Invalid(format!(
            "unknown change set version {}",
            changes.version
        )));
    }
    validate_paths(changes).map_err(CommitError::Invalid)?;
    let mut paths: Vec<&str> = Vec::new();
    for m in &changes.moves {
        paths.push(&m.from);
        paths.push(&m.to);
    }
    paths.extend(changes.writes.iter().map(|w| w.path.as_str()));
    paths.extend(changes.creates.iter().map(|w| w.path.as_str()));
    paths.extend(changes.deletes.iter().map(|d| d.path.as_str()));
    paths.extend(changes.remove_folders.iter().map(String::as_str));
    for p in paths {
        if goes_through_symlink(&store.root, p) {
            return Err(CommitError::Invalid(format!(
                "{p}: goes through a symbolic link"
            )));
        }
    }
    Ok(())
}

pub(crate) fn commit(
    store: &FolderStore,
    changes: &ChangeSet,
    verify: Option<Verify<'_>>,
) -> Result<usize, CommitError> {
    preflight(store, changes)?;

    // Stale? Nothing may have changed since the plan was made.
    let listing = store.list();
    let stale = textree::stale_paths(&listing.nodes, changes);
    if !stale.is_empty() {
        return Err(CommitError::Stale(stale));
    }

    // A destination that is taken is a conflict, never an overwrite.
    let mut conflicts = Vec::new();
    for m in &changes.moves {
        let to = under(&store.root, &m.to);
        let case_only = textree::path::fold(&m.from) == textree::path::fold(&m.to);
        if !case_only && fs::symlink_metadata(&to).is_ok() {
            conflicts.push(m.to.clone());
        }
    }
    for c in &changes.creates {
        if fs::symlink_metadata(under(&store.root, &c.path)).is_ok() {
            conflicts.push(c.path.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(CommitError::Conflict(conflicts));
    }

    let mut run = Run {
        root: &store.root,
        fail_at: store.fail_at,
        step: 0,
        undo: Vec::new(),
        set_aside: Vec::new(),
        unique: 0,
    };
    let outcome = apply(&mut run, store, changes).and_then(|()| match verify {
        Some(check) => check(store).map_err(|why| (String::new(), io::Error::other(why))),
        None => Ok(()),
    });
    match outcome {
        Ok(()) => {
            for hidden in &run.set_aside {
                let _ = if hidden.is_dir() {
                    fs::remove_dir_all(hidden)
                } else {
                    fs::remove_file(hidden)
                };
            }
            // The change is final. Folders it left empty go now; one that is not empty (a
            // hidden file in it, say) is left alone.
            for folder in &changes.remove_folders {
                let _ = fs::remove_dir(under(&store.root, folder));
            }
            Ok(changes.moves.len()
                + changes.creates.len()
                + changes.writes.len()
                + changes.deletes.len())
        }
        Err((path, e)) => {
            let problems = run.roll_back();
            let mut message = e.to_string();
            if problems.is_empty() {
                message.push_str(" (everything was put back)");
            } else {
                message.push_str(&format!(" (could not put back: {})", problems.join("; ")));
            }
            Err(CommitError::Failed { path, message })
        }
    }
}

fn apply(
    run: &mut Run<'_>,
    store: &FolderStore,
    changes: &ChangeSet,
) -> Result<(), (String, io::Error)> {
    let root = store.root.clone();
    for m in &changes.moves {
        let from = under(&root, &m.from);
        let to = under(&root, &m.to);
        if let Some(parent) = to.parent() {
            run.make_dirs(parent).map_err(|e| (m.to.clone(), e))?;
        }
        run.rename(&from, &to).map_err(|e| (m.to.clone(), e))?;
    }
    for c in &changes.creates {
        let path = under(&root, &c.path);
        if let Some(parent) = path.parent() {
            run.make_dirs(parent).map_err(|e| (c.path.clone(), e))?;
        }
        run.create(&path, c.content.as_bytes())
            .map_err(|e| (c.path.clone(), e))?;
    }
    for w in &changes.writes {
        run.write(&under(&root, &w.path), w.content.as_bytes())
            .map_err(|e| (w.path.clone(), e))?;
    }
    for d in &changes.deletes {
        run.set_aside_for_delete(&under(&root, &d.path))
            .map_err(|e| (d.path.clone(), e))?;
    }
    Ok(())
}
