use crate::pathsafe::is_within;
use crate::search::{IndexHandle, IndexState, SearchHit};
use crate::self_write::SelfWrites;
use crate::vault::{self, TreeNode};
use crate::watcher::{self, WatcherHandle};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tempfile::NamedTempFile;

/// Atomic file write: write to a temp file in the same directory, then rename to the target path.
/// Even if a crash/power loss happens mid-write, the target file is not truncated ("the FS is the truth").
fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent directory")
    })?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    // Flush down to physical storage, not just the OS buffer (fsync). Only then is the
    // content guaranteed after the rename even under power loss — persist alone has no durability.
    tmp.as_file().sync_all()?;
    // persist is a rename within the same volume, so it is atomic and replaces the existing file.
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Builds the `.textree/<rel>` sidecar path. `rel` is confined under `.textree/`, and
/// anything other than `Component::Normal` (parent refs, absolute paths, `.`) is rejected → no traversal.
fn sidecar_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() {
        return Err("sidecar path is empty".into());
    }
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err("invalid sidecar path (outside .textree)".into());
        }
    }
    Ok(root.join(".textree").join(rel_path))
}

/// Reads the `.textree/<rel>` sidecar. Returns `None` if absent (normal flow).
#[tauri::command]
pub fn read_sidecar(root: String, rel: String) -> Result<Option<String>, String> {
    let path = sidecar_path(Path::new(&root), &rel)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Atomically writes the `.textree/<rel>` sidecar (auto-creates the parent directory).
/// `.textree/` is ignored by the watcher (watcher::is_ignored), so self-write registration is unnecessary.
#[tauri::command]
pub fn write_sidecar(root: String, rel: String, content: String) -> Result<(), String> {
    let path = sidecar_path(Path::new(&root), &rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tree(root: String) -> Result<Vec<TreeNode>, String> {
    vault::build_tree(Path::new(&root)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_note(root: String, path: String) -> Result<String, String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    if !is_within(&root, &path) {
        return Err("path is outside the vault".into());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_note(
    root: String,
    path: String,
    content: String,
    self_writes: State<'_, Arc<SelfWrites>>,
    index: State<'_, Arc<IndexHandle>>,
) -> Result<(), String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    if !is_within(&root, &path) {
        return Err("path is outside the vault".into());
    }
    // Must register "just before" writing: if the watcher receives the event before
    // record runs right after the write hits disk, an echo loop forms (design §4.1).
    self_writes.record(&path, &content);
    match atomic_write(&path, &content) {
        Ok(()) => {
            // The watcher suppresses self-writes, so in-app edits update the index here.
            // An index failure does not fail the save (index = derived cache, graceful).
            if let Some(state) = index.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = state.index_note(&root, &path, &content);
            }
            Ok(())
        }
        Err(e) => {
            // On write failure the disk did not change, so remove the stale registration
            // to keep the registry from diverging from the actual disk state.
            self_writes.forget(&path);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub fn open_vault(
    root: String,
    app: AppHandle,
    self_writes: State<'_, Arc<SelfWrites>>,
    watcher_handle: State<'_, WatcherHandle>,
    index: State<'_, Arc<IndexHandle>>,
) -> Result<Vec<TreeNode>, String> {
    let root_path = PathBuf::from(&root);
    let tree = vault::build_tree(&root_path).map_err(|e| e.to_string())?;

    // Install the index (app data directory, per-vault hash). On failure only search is disabled —
    // graceful degradation (editing, tree, and file search remain intact without the index).
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = crate::search::index_dir(&app_data, &root_path);
    match IndexState::open_or_create(&dir) {
        Ok(state) => {
            let was_empty = state.is_empty().unwrap_or(true);
            *index.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
            if was_empty {
                // Full build in the background (non-blocking for the UI).
                let index_arc = index.inner().clone();
                let root_for_build = root_path.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(st) = index_arc.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                        let _ = st.rebuild(&root_for_build);
                    }
                });
            }
        }
        Err(e) => {
            eprintln!("failed to open index (search disabled): {e}");
            *index.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    // Explicitly drop the previous watcher (stop watching) before starting the new one, so that
    // on a vault switch leftover events from the old vault don't bleed into the new vault's processing.
    *watcher_handle.0.lock().unwrap() = None;
    let w = watcher::start(app, &root_path, self_writes.inner().clone(), index.inner().clone())?;
    *watcher_handle.0.lock().unwrap() = Some(w);

    Ok(tree)
}

// ── Structural edits (M4) — delegated to fs_ops ────────────────────────────────

#[tauri::command]
pub fn create_note(root: String, parent: String, name: String) -> Result<String, String> {
    let path = crate::fs_ops::create_note(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn create_folder(root: String, parent: String, name: String) -> Result<String, String> {
    let dir = crate::fs_ops::create_folder(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn promote_node(root: String, path: String) -> Result<String, String> {
    let dir = crate::fs_ops::promote_leaf(Path::new(&root), Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn delete_node(root: String, path: String) -> Result<(), String> {
    crate::fs_ops::delete_to_trash(Path::new(&root), Path::new(&path))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_node(root: String, path: String, name: String) -> Result<String, String> {
    let p = crate::fs_ops::rename_node(Path::new(&root), Path::new(&path), &name)
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn move_node(root: String, path: String, dest: String) -> Result<String, String> {
    let p = crate::fs_ops::move_node(Path::new(&root), Path::new(&path), Path::new(&dest))
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn adopt_node(root: String, path: String, leaf: String) -> Result<String, String> {
    let p = crate::fs_ops::adopt_into_leaf(Path::new(&root), Path::new(&path), Path::new(&leaf))
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

/// Saves an attached image. `data` is base64-encoded bytes. Returns the relative link to insert into the body.
#[tauri::command]
pub fn save_attachment(
    root: String,
    note: String,
    data: String,
    ext: String,
) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    crate::fs_ops::save_attachment(Path::new(&root), Path::new(&note), &bytes, &ext)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_content(
    query: String,
    limit: usize,
    index: State<'_, Arc<IndexHandle>>,
) -> Result<Vec<SearchHit>, String> {
    let guard = index.0.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(state) => state.search(&query, limit).map_err(|e| e.to_string()),
        None => Ok(Vec::new()), // no index → empty results (graceful)
    }
}

#[tauri::command]
pub fn rebuild_index(
    root: String,
    index: State<'_, Arc<IndexHandle>>,
) -> Result<(), String> {
    let root_path = PathBuf::from(root);
    let mut guard = index.0.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(state) => state.rebuild(&root_path).map_err(|e| e.to_string()),
        None => Err("index is not installed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sidecar_path_confines_to_dot_textree() {
        let root = Path::new("/vault");
        assert_eq!(
            sidecar_path(root, "favorites.json").unwrap(),
            Path::new("/vault/.textree/favorites.json")
        );
        assert_eq!(
            sidecar_path(root, "views/board.json").unwrap(),
            Path::new("/vault/.textree/views/board.json")
        );
        assert!(sidecar_path(root, "../secret").is_err());
        assert!(sidecar_path(root, "a/../../b").is_err());
        assert!(sidecar_path(root, "/etc/passwd").is_err());
        assert!(sidecar_path(root, "").is_err());
        assert!(sidecar_path(root, ".").is_err());
    }

    #[test]
    fn sidecar_write_then_read_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        assert_eq!(read_sidecar(root.clone(), "favorites.json".into()).unwrap(), None);
        write_sidecar(root.clone(), "favorites.json".into(), "[\"a.md\"]".into()).unwrap();
        assert_eq!(
            read_sidecar(root.clone(), "favorites.json".into()).unwrap(),
            Some("[\"a.md\"]".to_string())
        );
        write_sidecar(root.clone(), "views/b.json".into(), "{}".into()).unwrap();
        assert_eq!(read_sidecar(root.clone(), "views/b.json".into()).unwrap(), Some("{}".to_string()));
        assert!(write_sidecar(root.clone(), "../x".into(), "{}".into()).is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_content() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("note.md");
        std::fs::write(&f, "old").unwrap();
        atomic_write(&f, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_creates_when_absent() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("fresh.md");
        atomic_write(&f, "hi").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "hi");
    }
}
