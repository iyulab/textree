use crate::pathsafe::is_within;
use crate::search::{IndexHandle, IndexState, SearchHit};
use crate::self_write::SelfWrites;
use crate::vault::{self, TreeNode};
use crate::watcher::{self, WatcherHandle};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
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

const TRASH_MANIFEST: &str = "trash.json";

/// One trashed node's provenance. Lives in `.textree/trash.json` (sidecar, regeneratable
/// in spirit: if lost, the trash files themselves remain the truth — §1.4 / D17).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    /// Actual file/dir name inside `.textree/trash/` (collision-disambiguated).
    pub trash_name: String,
    /// Vault-root-relative original path, `/`-separated (portable across OS).
    pub original_rel: String,
    /// Unix epoch seconds at deletion.
    pub deleted_at: u64,
    pub is_dir: bool,
}

/// Reads `.textree/trash.json`. Absent or corrupt → empty (graceful; trash files are the truth).
fn read_trash_manifest(root: &Path) -> Vec<TrashItem> {
    let path = match sidecar_path(root, TRASH_MANIFEST) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Atomically rewrites the manifest (temp→fsync→rename via atomic_write).
fn write_trash_manifest(root: &Path, items: &[TrashItem]) -> Result<(), String> {
    let path = sidecar_path(root, TRASH_MANIFEST)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
    atomic_write(&path, &json).map_err(|e| e.to_string())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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

/// Vault-root-relative, `/`-separated path of an existing target (for manifest portability).
fn rel_to_root(root: &Path, target: &Path) -> Result<String, String> {
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let target_c = target.canonicalize().map_err(|e| e.to_string())?;
    target_c
        .strip_prefix(&root_c)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.replace('\\', "/"))
        .ok_or_else(|| "target is not within the vault".to_string())
}

#[tauri::command]
pub fn delete_node(root: String, path: String) -> Result<(), String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    // Capture provenance before the move (canonicalize needs the path to still exist).
    let original_rel = rel_to_root(root_p, target)?;
    let is_dir = target.is_dir();
    // Move first (fs_ops validates is_within / root / .textree). Manifest after — a mid-crash
    // leaves an "unknown-origin" trash file (recoverable) rather than a dangling manifest entry.
    let dest = crate::fs_ops::delete_to_trash(root_p, target).map_err(|e| e.to_string())?;
    let trash_name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "cannot read trashed name".to_string())?
        .to_string();
    let mut items = read_trash_manifest(root_p);
    items.push(TrashItem { trash_name, original_rel, deleted_at: now_secs(), is_dir });
    write_trash_manifest(root_p, &items)
}

/// Re-validates a vault-relative path from the (user-editable) manifest: every component must be
/// Normal and a valid name. "Security at the boundary" — the manifest is not trusted input.
fn validate_trash_rel(rel: &str) -> Result<(), String> {
    let p = Path::new(rel);
    let mut any = false;
    for comp in p.components() {
        match comp {
            Component::Normal(s) => {
                any = true;
                if !crate::pathsafe::is_valid_name(&s.to_string_lossy()) {
                    return Err("invalid path segment in trash entry".into());
                }
            }
            _ => return Err("invalid trash path (non-normal component)".into()),
        }
    }
    if !any {
        return Err("empty trash path".into());
    }
    Ok(())
}

#[tauri::command]
pub fn restore_node(root: String, trash_name: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    // trash_name must be a single safe segment (rejects separators / .. / dotfiles).
    if !crate::pathsafe::is_valid_name(&trash_name) {
        return Err("invalid trash name".into());
    }
    let trash_path = root_p.join(".textree").join("trash").join(&trash_name);
    if !trash_path.exists() {
        return Err("trash item not found".into());
    }
    let mut items = read_trash_manifest(root_p);
    let idx = items.iter().position(|it| it.trash_name == trash_name);
    let original_rel = match idx {
        Some(i) => {
            let rel = items[i].original_rel.clone();
            validate_trash_rel(&rel)?; // boundary recheck on untrusted manifest
            rel
        }
        None => trash_name.clone(), // unknown origin → restore to vault root (§3 fallback)
    };
    let restored = crate::fs_ops::restore_from_trash(root_p, &trash_path, &original_rel)
        .map_err(|e| e.to_string())?;
    // Rename succeeded → now drop the manifest entry (rename-first ordering).
    if let Some(i) = idx {
        items.remove(i);
        write_trash_manifest(root_p, &items)?;
    }
    rel_to_root(root_p, &restored)
}

#[tauri::command]
pub fn list_trash(root: String) -> Result<Vec<TrashItem>, String> {
    let root_p = Path::new(&root);
    let trash_dir = root_p.join(".textree").join("trash");
    let mut items = read_trash_manifest(root_p);
    // Drop stale entries (manifest points to a file the user removed externally).
    items.retain(|it| trash_dir.join(&it.trash_name).exists());
    // Surface orphan files (in trash dir but absent from the manifest) as unknown-origin.
    let known: std::collections::HashSet<String> =
        items.iter().map(|it| it.trash_name.clone()).collect();
    if let Ok(entries) = std::fs::read_dir(&trash_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !known.contains(&name) {
                let is_dir = e.path().is_dir();
                items.push(TrashItem { trash_name: name.clone(), original_rel: name, deleted_at: 0, is_dir });
            }
        }
    }
    Ok(items)
}

#[tauri::command]
pub fn purge_trash(root: String, trash_name: Option<String>) -> Result<(), String> {
    let root_p = Path::new(&root);
    let trash_dir = root_p.join(".textree").join("trash");
    match trash_name {
        Some(name) => {
            if !crate::pathsafe::is_valid_name(&name) {
                return Err("invalid trash name".into());
            }
            let p = trash_dir.join(&name);
            if p.is_dir() {
                std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
            } else if p.exists() {
                std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            }
            let mut items = read_trash_manifest(root_p);
            items.retain(|it| it.trash_name != name);
            write_trash_manifest(root_p, &items)
        }
        None => {
            if trash_dir.exists() {
                std::fs::remove_dir_all(&trash_dir).map_err(|e| e.to_string())?;
            }
            write_trash_manifest(root_p, &[])
        }
    }
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

/// Resolves how to invoke canopy. Dev/E2E: the `TEXTREE_CANOPY_CLI` env var (path to the CLI script
/// or exe) — a `.js` path is run via `node`. Production: the bundled single-exe sidecar in the
/// resource dir (its build/bundling lands in a later cycle).
fn resolve_canopy(app: &AppHandle) -> Result<crate::publish::CanopyInvocation, String> {
    use crate::publish::CanopyInvocation;
    if let Ok(p) = std::env::var("TEXTREE_CANOPY_CLI") {
        let path = PathBuf::from(&p);
        if path.extension().and_then(|e| e.to_str()) == Some("js") {
            return Ok(CanopyInvocation {
                program: "node".into(),
                prefix_args: vec![path.into_os_string()],
            });
        }
        return Ok(CanopyInvocation { program: path.into_os_string(), prefix_args: vec![] });
    }
    let resource = app.path().resource_dir().map_err(|e| e.to_string())?;
    let exe = resource.join(if cfg!(windows) { "canopy.exe" } else { "canopy" });
    if exe.exists() {
        return Ok(CanopyInvocation { program: exe.into_os_string(), prefix_args: vec![] });
    }
    Err("the canopy renderer is not available (set TEXTREE_CANOPY_CLI in dev, or bundle the sidecar)"
        .into())
}

/// Publishes the open vault to a static site by spawning canopy. Read-only over the source (D13):
/// the vault `.md` is never mutated; only `out_dir` (which must lie outside the vault) is written.
#[tauri::command]
pub fn publish_site(
    app: AppHandle,
    vault_path: String,
    out_dir: String,
    options: crate::publish::PublishOptions,
) -> Result<crate::publish::PublishResult, String> {
    let vault = PathBuf::from(&vault_path);
    let out = PathBuf::from(&out_dir);
    let canopy = resolve_canopy(&app)?;
    crate::publish::run_publish(&vault, &out, &options, &canopy)
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

    #[test]
    fn trash_manifest_roundtrips_and_is_empty_when_absent() {
        let tmp = TempDir::new().unwrap();
        // Absent manifest reads as empty (graceful — the FS is the truth).
        assert!(read_trash_manifest(tmp.path()).is_empty());

        let items = vec![TrashItem {
            trash_name: "memo (1).md".into(),
            original_rel: "refs/memo.md".into(),
            deleted_at: 1718600000,
            is_dir: false,
        }];
        write_trash_manifest(tmp.path(), &items).unwrap();
        let read = read_trash_manifest(tmp.path());
        assert_eq!(read, items);
        // Written under .textree/ (sidecar).
        assert!(tmp.path().join(".textree").join("trash.json").is_file());
    }

    #[test]
    fn trash_manifest_corrupt_reads_as_empty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".textree");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("trash.json"), "{ not valid json").unwrap();
        // Corrupt manifest must not crash — degrade to empty (trash files remain the truth).
        assert!(read_trash_manifest(tmp.path()).is_empty());
    }

    #[test]
    fn restore_node_roundtrip_and_clears_manifest() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("memo.md");
        std::fs::write(&note, "x").unwrap();
        delete_node(tmp.path().to_string_lossy().into(), note.to_string_lossy().into()).unwrap();
        let trash_name = read_trash_manifest(tmp.path())[0].trash_name.clone();

        let restored = restore_node(tmp.path().to_string_lossy().into(), trash_name).unwrap();
        assert_eq!(restored, "memo.md");
        assert!(note.is_file(), "back at original location");
        assert!(read_trash_manifest(tmp.path()).is_empty(), "manifest entry removed");
    }

    #[test]
    fn restore_node_rejects_tampered_original_rel() {
        let tmp = TempDir::new().unwrap();
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(trash.join("evil.md"), "x").unwrap();
        // A hand-edited manifest tries to escape the vault via the original_rel.
        let items = vec![TrashItem {
            trash_name: "evil.md".into(),
            original_rel: "../escape.md".into(),
            deleted_at: 0,
            is_dir: false,
        }];
        write_trash_manifest(tmp.path(), &items).unwrap();

        let res = restore_node(tmp.path().to_string_lossy().into(), "evil.md".into());
        assert!(res.is_err(), "path traversal in original_rel must be rejected at the boundary");
    }

    #[test]
    fn restore_node_unknown_origin_goes_to_root() {
        let tmp = TempDir::new().unwrap();
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(trash.join("orphan.md"), "x").unwrap(); // no manifest entry

        let restored = restore_node(tmp.path().to_string_lossy().into(), "orphan.md".into()).unwrap();
        assert_eq!(restored, "orphan.md");
        assert!(tmp.path().join("orphan.md").is_file());
    }

    #[test]
    fn delete_node_records_provenance_in_manifest() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("refs");
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("memo.md");
        std::fs::write(&note, "x").unwrap();

        delete_node(tmp.path().to_string_lossy().into(), note.to_string_lossy().into()).unwrap();

        let items = read_trash_manifest(tmp.path());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].original_rel, "refs/memo.md"); // forward slashes, vault-relative
        assert!(!items[0].is_dir);
        assert!(!note.exists(), "original moved to trash");
        // The recorded trash_name actually exists in the trash dir.
        let trashed = tmp.path().join(".textree").join("trash").join(&items[0].trash_name);
        assert!(trashed.is_file());
    }

    #[test]
    fn list_trash_merges_manifest_and_orphans_drops_stale() {
        let tmp = TempDir::new().unwrap();
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(trash.join("known.md"), "x").unwrap();
        std::fs::write(trash.join("orphan.md"), "x").unwrap(); // on disk, not in manifest
        let items = vec![
            TrashItem { trash_name: "known.md".into(), original_rel: "known.md".into(), deleted_at: 5, is_dir: false },
            TrashItem { trash_name: "ghost.md".into(), original_rel: "ghost.md".into(), deleted_at: 9, is_dir: false }, // stale: no file
        ];
        write_trash_manifest(tmp.path(), &items).unwrap();

        let listed = list_trash(tmp.path().to_string_lossy().into()).unwrap();
        let names: std::collections::HashSet<_> = listed.iter().map(|i| i.trash_name.as_str()).collect();
        assert!(names.contains("known.md"));
        assert!(names.contains("orphan.md"), "orphan file surfaced");
        assert!(!names.contains("ghost.md"), "stale manifest entry dropped");
    }

    #[test]
    fn purge_individual_and_all() {
        let tmp = TempDir::new().unwrap();
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(trash.join("a.md"), "x").unwrap();
        std::fs::write(trash.join("b.md"), "x").unwrap();
        write_trash_manifest(tmp.path(), &[
            TrashItem { trash_name: "a.md".into(), original_rel: "a.md".into(), deleted_at: 0, is_dir: false },
            TrashItem { trash_name: "b.md".into(), original_rel: "b.md".into(), deleted_at: 0, is_dir: false },
        ]).unwrap();

        purge_trash(tmp.path().to_string_lossy().into(), Some("a.md".into())).unwrap();
        assert!(!trash.join("a.md").exists());
        assert_eq!(read_trash_manifest(tmp.path()).len(), 1);

        purge_trash(tmp.path().to_string_lossy().into(), None).unwrap();
        assert!(!trash.join("b.md").exists());
        assert!(read_trash_manifest(tmp.path()).is_empty());
    }
}
