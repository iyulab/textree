//! A read port over a folder on disk.
//!
//! Every Markdown file (`.md`) is a document, every other file is an asset, every folder is a
//! collection. Names that start with a dot (`.git`, `.obsidian`) are skipped, and symbolic
//! links to folders are not followed. The folder is only ever read.

use std::fs;
use std::path::{Path, PathBuf};

use textree::{
    content_version, is_document_path, ChangeSet, CommitError, Issue, Listing, Node, NodeKind,
    ReadPort, WritePort,
};

mod commit;
pub mod edit;

/// A hierarchy of documents stored in a folder.
#[derive(Clone, Debug)]
pub struct FolderStore {
    root: PathBuf,
    /// For tests: make the n-th step of a commit fail, to prove that a failure is undone.
    fail_at: Option<usize>,
}

impl FolderStore {
    /// Opens a folder. Fails when the path is not a readable folder.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        match fs::metadata(&root) {
            Ok(meta) if meta.is_dir() => Ok(FolderStore {
                root,
                fail_at: None,
            }),
            Ok(_) => Err(format!("{}: not a folder", root.display())),
            Err(e) => Err(format!("{}: {e}", root.display())),
        }
    }

    /// Makes the `n`-th mutating step of every commit fail (counting from 1). For tests of the
    /// undo path; a store used for real never sets it.
    pub fn with_failure_at(mut self, n: usize) -> Self {
        self.fail_at = Some(n);
        self
    }

    fn walk(&self, dir: &Path, rel: &str, out: &mut Listing) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                out.issues.push(Issue {
                    path: rel.to_string(),
                    message: format!("cannot list folder: {e}"),
                });
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    out.issues.push(Issue {
                        path: rel.to_string(),
                        message: format!("cannot list folder: {e}"),
                    });
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                let shown = entry.file_name().to_string_lossy().into_owned();
                out.issues.push(Issue {
                    path: join(rel, &shown),
                    message: "cannot read: name is not valid UTF-8".to_string(),
                });
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = join(rel, name);
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                out.nodes.push(Node {
                    path: path.clone(),
                    kind: NodeKind::Collection,
                    version: String::new(),
                });
                self.walk(&entry.path(), &path, out);
            } else if file_type.is_file() || file_type.is_symlink() {
                // A symbolic link to a folder is skipped; one to a file is read like the file.
                match fs::metadata(entry.path()) {
                    Ok(meta) if meta.is_dir() => continue,
                    Ok(meta) if is_document_path(&path) => {
                        let version = match fs::read(entry.path()) {
                            Ok(bytes) => content_version(&bytes),
                            Err(_) => "unreadable".to_string(),
                        };
                        let _ = meta;
                        out.nodes.push(Node {
                            path,
                            kind: NodeKind::Document,
                            version,
                        });
                    }
                    Ok(meta) => {
                        out.nodes.push(Node {
                            path,
                            kind: NodeKind::Asset,
                            version: format!("size:{}", meta.len()),
                        });
                    }
                    Err(e) => out.issues.push(Issue {
                        path,
                        message: format!("cannot read: {e}"),
                    }),
                }
            }
        }
    }
}

fn join(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

impl ReadPort for FolderStore {
    fn list(&self) -> Listing {
        let mut listing = Listing::default();
        self.walk(&self.root, "", &mut listing);
        listing
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut full = self.root.clone();
        for part in path.split('/') {
            if part.is_empty() || part == ".." || part == "." {
                return Err(format!("not a path inside the folder: {path}"));
            }
            full.push(part);
        }
        fs::read(&full).map_err(|e| e.to_string())
    }
}

impl FolderStore {
    /// Everything that can be told about a change set without changing the folder: it is well
    /// formed, stays inside the folder and goes through no symbolic link.
    pub fn preflight(&self, changes: &ChangeSet) -> Result<(), CommitError> {
        commit::preflight(self, changes)
    }

    /// Like [`WritePort::commit`], and `verify` looks at the folder after the changes are made
    /// but before they are final: if it says no, everything is put back.
    pub fn commit_checked(
        &self,
        changes: &ChangeSet,
        verify: &dyn Fn(&FolderStore) -> Result<(), String>,
    ) -> Result<usize, CommitError> {
        commit::commit(self, changes, Some(verify))
    }
}

impl WritePort for FolderStore {
    fn commit(&self, changes: &ChangeSet) -> Result<usize, CommitError> {
        commit::commit(self, changes, None)
    }
}
