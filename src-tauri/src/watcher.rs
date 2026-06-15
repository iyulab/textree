//! External file change watching (M3).
//!
//! Recursively watches the vault root with `notify-debouncer-full`, filters out
//! the app's own writes (echoes) via the self-write registry, then emits
//! `fs_changed` events to the frontend. Dropping the debouncer stops watching,
//! so the returned handle is kept alive in state.

use crate::search::IndexHandle;
use crate::self_write::SelfWrites;
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, Debouncer, RecommendedCache,
};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
}

/// Change payload sent to the frontend (app IPC wire type — separate from library types).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct FsChange {
    pub kind: ChangeKind,
    pub path: String,
}

pub type VaultWatcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// Handle to the running vault watcher. Managed as a Tauri `State`.
/// Opening a new vault drops the previous debouncer (stops watching) and replaces it.
#[derive(Default)]
pub struct WatcherHandle(pub Mutex<Option<VaultWatcher>>);

/// Reduces a notify `EventKind` to our change kind. Kinds we don't care about return `None`.
fn map_kind(kind: &EventKind) -> Option<ChangeKind> {
    if kind.is_create() {
        Some(ChangeKind::Created)
    } else if kind.is_remove() {
        Some(ChangeKind::Removed)
    } else if kind.is_modify() {
        Some(ChangeKind::Modified)
    } else {
        None // ignore Access/Any/Other
    }
}

/// Dot segments relative to the vault root (`.textree`, `.git`, etc.) are hidden in the
/// tree, so the watcher ignores them too. Dots in the root path itself are not counted
/// toward the ignore decision.
fn is_ignored(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')))
}

/// Converts a single `(kind, path)` into a frontend change. Returns `None` (echo
/// suppression) when it's a create/modify and the current file content matches a
/// registered self-write.
fn to_fs_change(kind: ChangeKind, path: &Path, self_writes: &SelfWrites) -> Option<FsChange> {
    if matches!(kind, ChangeKind::Created | ChangeKind::Modified) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                if self_writes.take_if_matches(path, &content) {
                    return None; // self-write echo — suppress
                }
            }
            Err(_) => {
                // If the content can't be read, an echo decision is impossible. Clean up
                // any remaining registration so a stale entry doesn't wrongly suppress a
                // later external change with identical content.
                self_writes.forget(path);
            }
        }
    }
    Some(FsChange {
        kind,
        path: path.display().to_string(),
    })
}

/// Applies a batch of changes to the index and commits once (.md only; excluding dot
/// paths is already done by the caller's is_ignored). No-op when no index is installed.
pub fn apply_changes_to_index(handle: &IndexHandle, root: &Path, changes: &[FsChange]) {
    let mut guard = handle.0.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.as_mut() else { return };
    let mut touched = false;
    for c in changes {
        let p = Path::new(&c.path);
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            continue;
        }
        state.apply_change(root, p, matches!(c.kind, ChangeKind::Removed));
        touched = true;
    }
    if touched {
        let _ = state.commit();
    }
}

/// Starts a debouncer that recursively watches the vault root.
pub fn start(
    app: AppHandle,
    root: &Path,
    self_writes: Arc<SelfWrites>,
    index: Arc<IndexHandle>,
) -> Result<VaultWatcher, String> {
    let root_owned = root.to_path_buf();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else {
                return; // ignore watcher errors — recovers naturally on the next event
            };
            let mut batch: Vec<FsChange> = Vec::new();
            for ev in events {
                let Some(kind) = map_kind(&ev.kind) else {
                    continue;
                };
                for path in &ev.paths {
                    if is_ignored(&root_owned, path) {
                        continue;
                    }
                    if let Some(change) = to_fs_change(kind.clone(), path, &self_writes) {
                        let _ = app.emit("fs_changed", change.clone());
                        batch.push(change);
                    }
                }
            }
            apply_changes_to_index(&index, &root_owned, &batch);
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    Ok(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify_debouncer_full::notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn map_kind_classifies_create_modify_remove_and_ignores_access() {
        assert_eq!(
            map_kind(&EventKind::Create(CreateKind::Any)),
            Some(ChangeKind::Created)
        );
        assert_eq!(
            map_kind(&EventKind::Modify(ModifyKind::Any)),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            map_kind(&EventKind::Remove(RemoveKind::Any)),
            Some(ChangeKind::Removed)
        );
        assert_eq!(map_kind(&EventKind::Access(AccessKind::Any)), None);
    }

    #[test]
    fn is_ignored_skips_dot_segments_below_root() {
        let root = PathBuf::from("/vault");
        assert!(is_ignored(&root, &PathBuf::from("/vault/.textree/db.json")));
        assert!(is_ignored(&root, &PathBuf::from("/vault/.git/config")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/노트.md")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/일기/2026.md")));
    }

    #[test]
    fn is_ignored_does_not_count_dot_in_root_path() {
        let root = PathBuf::from("/home/.config/vault");
        // .config inside the root is not subject to the ignore decision.
        assert!(!is_ignored(&root, &PathBuf::from("/home/.config/vault/노트.md")));
        // A dot folder below the root is still ignored.
        assert!(is_ignored(
            &root,
            &PathBuf::from("/home/.config/vault/.textree/x.json")
        ));
    }

    #[test]
    fn self_write_is_suppressed_external_change_is_emitted() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "self-written").unwrap();
        let sw = SelfWrites::default();
        sw.record(&f, "self-written");

        // Content matches the registered self-write → suppress.
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_none());

        // An external tool changes it to different content → emit.
        std::fs::write(&f, "external edit").unwrap();
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
    }

    #[test]
    fn removed_is_always_emitted() {
        let sw = SelfWrites::default();
        let change = to_fs_change(ChangeKind::Removed, &PathBuf::from("/v/gone.md"), &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Removed));
    }

    use crate::search::{IndexHandle, IndexState};
    use std::sync::Arc;

    #[test]
    fn apply_changes_to_index_upserts_and_deletes() {
        let vault = TempDir::new().unwrap();
        let idx = TempDir::new().unwrap();
        std::fs::write(vault.path().join("a.md"), "처음 본문").unwrap();

        let handle: Arc<IndexHandle> = Arc::new(IndexHandle::default());
        *handle.0.lock().unwrap() = Some(IndexState::open_or_create(idx.path()).unwrap());

        // created/modified → upsert
        let changes = vec![
            FsChange { kind: ChangeKind::Created, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            let st = guard.as_ref().unwrap();
            assert_eq!(st.search("본문", 10).unwrap().len(), 1);
        }

        // removed → delete
        let changes = vec![
            FsChange { kind: ChangeKind::Removed, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            assert_eq!(guard.as_ref().unwrap().search("본문", 10).unwrap().len(), 0);
        }
    }

    #[test]
    fn unreadable_modified_path_emits_and_forgets_stale_entry() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("vanished.md");
        let sw = SelfWrites::default();
        sw.record(&f, "was-self-written");
        // File doesn't exist so the read fails → the event passes through (Some), the registration is cleaned up.
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
        // Since it was consumed by forget, the same content arriving again is not treated as a self-write.
        std::fs::write(&f, "was-self-written").unwrap();
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_some());
    }
}
