use crate::host;
use crate::pathsafe::is_within;
use crate::search::{IndexHandle, IndexState, SearchHit};
use crate::self_write::SelfWrites;
use crate::vault::{self, TreeNode};
use crate::watcher::WatcherHandle;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use tempfile::NamedTempFile;

/// Name of the staging directory for atomic writes, inside the repository's own storage.
const TEMP_DIR_NAME: &str = "textree-tmp";

/// Legacy staging location, kept only so leftovers from earlier versions get cleaned up.
const LEGACY_TEMP_DIR: [&str; 2] = [".textree", "tmp"];

/// Where atomic writes stage their temp file.
///
/// Repository storage is the right home for it: it is on the same volume as the target (so
/// `persist` is still an atomic rename), it is not part of anyone's working tree (so transient
/// `.tmpXXXX` files never show up as changes, not even when the folder sits inside a
/// repository someone else uses), and it leaves no trace in the folder itself. A folder with no
/// repository at all falls back to the older location so writing still works.
fn temp_dir(root: &Path) -> PathBuf {
    match crate::git_engine::git_dir(root) {
        Some(git) => git.join(TEMP_DIR_NAME),
        None => root.join(LEGACY_TEMP_DIR[0]).join(LEGACY_TEMP_DIR[1]),
    }
}

/// Best-effort removal of orphaned temp files — e.g. a crash or power loss between create and
/// rename. Without this they would linger forever. Called on vault open. Errors are ignored (an
/// in-flight temp held open by another instance simply stays).
///
/// The older location is swept as well, so upgrading leaves nothing behind.
fn clear_temp_dir(root: &Path) {
    let legacy = root.join(LEGACY_TEMP_DIR[0]).join(LEGACY_TEMP_DIR[1]);
    for dir in [temp_dir(root), legacy] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Atomic file write: write to a temp file under `<root>/.textree/tmp/`, then rename to the target.
/// Even if a crash/power loss happens mid-write, the target file is not truncated ("the FS is the truth").
fn atomic_write(root: &Path, path: &Path, content: &str) -> io::Result<()> {
    atomic_write_bytes(root, path, content.as_bytes())
}

/// The same guarantee for content that is not necessarily text — a restored file can be
/// anything that was kept alongside the notes.
fn atomic_write_bytes(root: &Path, path: &Path, content: &[u8]) -> io::Result<()> {
    let dir = temp_dir(root);
    std::fs::create_dir_all(&dir)?;
    let mut tmp = NamedTempFile::new_in(&dir)?;
    tmp.write_all(content)?;
    // Flush down to physical storage, not just the OS buffer (fsync). Only then is the
    // content guaranteed after the rename even under power loss — persist alone has no durability.
    tmp.as_file().sync_all()?;
    // persist is a rename within the same volume (both under the vault root), so it is atomic and
    // replaces the existing file.
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Seed note written into a freshly created default vault. English only (public repo).
const WELCOME_MD: &str = "---\ntitle: Welcome to Textree\nicon: 🌳\n---\n\n# Welcome to Textree\n\nThis is your vault — a plain folder of Markdown files on your own disk.\nNo account, no lock-in. What you see here is the default look, with no setup.\n\n## A few things to try\n- Edit this note. Everything is just `.md` you fully own.\n- Create notes and folders from the toolbar on the left.\n- **Already keep notes in another folder?** Click 📁 in the sidebar to open\n  any existing standard Markdown vault.\n\nYou can delete this note anytime.\n";

/// True if `dir` directly contains at least one `*.md` file.
fn has_markdown(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().any(|e| {
            e.path().extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("md"))
                == Some(true)
        }),
        Err(_) => false,
    }
}

/// Ensures `base/Textree/` exists and, only when it has no Markdown yet, seeds `welcome.md`
/// atomically. Never overwrites existing content (non-destructive). Returns the vault path.
fn ensure_vault_at(base: &Path) -> Result<PathBuf, (String, Option<i32>)> {
    let vault = base.join("Textree");
    std::fs::create_dir_all(&vault).map_err(|e| (e.to_string(), e.raw_os_error()))?;
    if !has_markdown(&vault) {
        atomic_write(&vault, &vault.join("welcome.md"), WELCOME_MD)
            .map_err(|e| (e.to_string(), e.raw_os_error()))?;
    }
    Ok(vault)
}

/// Ordered candidate base directories under which the default `Textree/` vault may live.
/// Dev/E2E: a single forced `TEXTREE_DEFAULT_VAULT_BASE` (test isolation, mirrors TEXTREE_CANOPY_CLI;
/// no fallback so tests stay deterministic). Production: Documents first (preferred), then the home
/// dir, then app-local-data as a last resort. Documents can resolve to an *invalid* path (e.g. a
/// OneDrive-redirected/uninitialized Known Folder) that exists in name but cannot be created under —
/// so we keep going past it rather than trusting the first resolved path.
fn candidate_vault_bases(app: &AppHandle) -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("TEXTREE_DEFAULT_VAULT_BASE") {
        if !p.is_empty() {
            return vec![PathBuf::from(p)];
        }
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !bases.contains(&dir) {
            bases.push(dir);
        }
    };
    if let Ok(d) = app.path().document_dir() {
        push(d);
    }
    if let Ok(h) = app.path().home_dir() {
        push(h);
    }
    if let Ok(a) = app.path().app_local_data_dir() {
        push(a);
    }
    bases
}

/// Tries each candidate base in order, returning the first under which a vault was successfully
/// created/seeded. `fell_back` is true when that base was not the first (preferred) candidate —
/// i.e. the Documents location was unusable and the vault landed elsewhere, which the UI surfaces
/// so the user always knows where their notes live (data sovereignty). Errors only when *every*
/// candidate fails — never silently no-ops into a blank app.
fn first_creatable_vault(bases: &[PathBuf]) -> Result<(PathBuf, bool), (String, Option<i32>)> {
    let mut last_err = ("no candidate base directory could be resolved".to_string(), None);
    for (i, base) in bases.iter().enumerate() {
        match ensure_vault_at(base) {
            Ok(vault) => return Ok((vault, i > 0)),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Result of resolving the default vault: its absolute path plus whether the preferred (Documents)
/// location was unusable so the vault was created in a fallback location instead.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultVault {
    pub path: String,
    pub fell_back: bool,
}

/// Ensures the default vault exists (creating it and seeding `welcome.md` on first run) and
/// returns its absolute path. The single backend owner of OS-path resolution + seeding. Falls
/// back through home → app-local-data when Documents is unusable so first run never blank-screens.
#[tauri::command]
pub fn ensure_default_vault(app: AppHandle) -> Result<DefaultVault, String> {
    let bases = candidate_vault_bases(&app);
    match first_creatable_vault(&bases) {
        Ok((vault, fell_back)) => {
            if fell_back {
                log::warn!("ensure_default_vault: preferred base unusable, fell back to {}", vault.display());
            } else {
                log::info!("ensure_default_vault: {}", vault.display());
            }
            Ok(DefaultVault { path: vault.display().to_string(), fell_back })
        }
        Err((msg, os_error_code)) => {
            crate::telemetry::emit(crate::telemetry::event::TelemetryEvent::VaultOpenFailed { os_error_code });
            Err(msg)
        }
    }
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
    atomic_write(root, &path, &json).map_err(|e| e.to_string())
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
    let root_p = Path::new(&root);
    let path = sidecar_path(root_p, &rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(root_p, &path, &content).map_err(|e| e.to_string())
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
        log::warn!("read_note: rejected unsafe path: {}", path.display());
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
    host: State<'_, Arc<host::HostHandle>>,
) -> Result<(), String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    if !is_within(&root, &path) {
        log::warn!("write_note: rejected unsafe path: {}", path.display());
        return Err("path is outside the vault".into());
    }
    // Must register "just before" writing: if the watcher receives the event before
    // record runs right after the write hits disk, an echo loop forms (design §4.1).
    self_writes.record(&path, &content);
    match atomic_write(&root, &path, &content) {
        Ok(()) => {
            log::info!("write_note: {} ({} bytes)", path.display(), content.len());
            // The watcher suppresses self-writes, so in-app edits update the index here.
            // An index failure does not fail the save (index = derived cache, graceful).
            if let Some(state) = index.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = state.index_note(&root, &path, &content);
            }
            // Background semantic index (content-immutable; auto-allowed per D18).
            let host_arc = host.inner().clone();
            let (v, p) = (root.to_string_lossy().to_string(), path.to_string_lossy().to_string());
            tauri::async_runtime::spawn_blocking(move || {
                host::index_note(&host_arc, &v, &p);
            });
            Ok(())
        }
        Err(e) => {
            // On write failure the disk did not change, so remove the stale registration
            // to keep the registry from diverging from the actual disk state.
            self_writes.forget(&path);
            log::error!("write_note failed for {}: {}", path.display(), e);
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
    host: State<'_, Arc<host::HostHandle>>,
) -> Result<Vec<TreeNode>, String> {
    let root_path = PathBuf::from(&root);
    // Sweep orphaned atomic-write temps (crash/power-loss leftovers) so they don't linger and sync.
    clear_temp_dir(&root_path);
    let tree = vault::build_tree(&root_path).map_err(|e| e.to_string())?;
    log::info!("open_vault: {} ({} top-level nodes)", root, tree.len());

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
            log::warn!("failed to open index (search disabled): {e}");
            *index.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    // Background semantic reindex (content-immutable; auto-allowed per D18).
    host.set_current_vault(root.clone());
    let host_arc = host.inner().clone();
    let vault_str = root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        host::reindex_vault(&host_arc, &vault_str);
    });

    // Explicitly drop the previous watchdog (stops its thread and the debouncer) before
    // starting the new one, so leftover events from the old vault don't bleed into the new.
    *watcher_handle.0.lock().unwrap() = None;
    let wd = crate::liveness::Watchdog::spawn(
        app,
        &root_path,
        self_writes.inner().clone(),
        index.inner().clone(),
    )?;
    *watcher_handle.0.lock().unwrap() = Some(wd);

    Ok(tree)
}

// ── Structural edits (M4) — delegated to fs_ops ────────────────────────────────

#[tauri::command]
pub fn create_note(root: String, parent: String, name: String) -> Result<String, String> {
    let path = crate::fs_ops::create_note(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    log::info!("create_note: {}", path.display());
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn create_untitled_note(root: String, parent: String) -> Result<String, String> {
    let path = crate::fs_ops::create_untitled_note(Path::new(&root), Path::new(&parent))
        .map_err(|e| e.to_string())?;
    log::info!("create_untitled_note: {}", path.display());
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn create_note_with_content(
    root: String,
    parent: String,
    name: String,
    content: String,
) -> Result<String, String> {
    let path = crate::fs_ops::create_note_with_content(
        Path::new(&root),
        Path::new(&parent),
        &name,
        &content,
    )
    .map_err(|e| e.to_string())?;
    log::info!("create_note_with_content: {}", path.display());
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn rename_note_unique(root: String, path: String, name: String) -> Result<String, String> {
    let p = crate::fs_ops::rename_note_unique(Path::new(&root), Path::new(&path), &name)
        .map_err(|e| e.to_string())?;
    log::info!("rename_note_unique: {} -> {}", path, p.display());
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn create_folder(root: String, parent: String, name: String) -> Result<String, String> {
    let dir = crate::fs_ops::create_folder(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    log::info!("create_folder: {}", dir.display());
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn promote_node(root: String, path: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    // Resolved first: promoting replaces the file this names.
    let from_rel = rel_to_root(root_p, Path::new(&path)).ok();
    let dir = crate::fs_ops::promote_leaf(root_p, Path::new(&path)).map_err(|e| e.to_string())?;
    // The note did not disappear, it went one level down and took its history with it.
    if let (Some(from), Some(to)) = (from_rel, folder_note_rel(root_p, &dir)) {
        record_moves(root_p, &[(from, to)]);
    }
    Ok(dir.display().to_string())
}

/// The vault-relative path of the note a folder is named after.
fn folder_note_rel(root: &Path, dir: &Path) -> Option<String> {
    let stem = dir.file_name().and_then(|s| s.to_str())?;
    let dir_rel = rel_to_root(root, dir).ok()?;
    Some(format!("{dir_rel}/{stem}.md"))
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

/// Every file at or under `target`, as (path on disk, path within the repository) pairs.
fn files_under(
    prepared: &crate::git_engine::VaultRepo,
    root: &Path,
    target: &Path,
) -> Vec<(PathBuf, PathBuf)> {
    let mut found = Vec::new();
    let mut stack = vec![target.to_path_buf()];
    while let Some(current) = stack.pop() {
        if current.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
            continue;
        }
        let Ok(rel) = rel_to_root(root, &current) else {
            continue;
        };
        found.push((current, prepared.path_in_repo(Path::new(&rel))));
    }
    found
}

/// Keeps the state of everything about to be deleted, and records that it happened.
///
/// Everything is kept, not only what history lacks. Two reasons, and each alone is enough:
///
/// - Being recorded once says nothing about the state on disk now. A note recorded, then edited
///   without being recorded again, then deleted, has its newer state nowhere else — skipping it
///   because the path appears in history loses exactly the work that was never saved.
/// - The revision is also the only record of *when* something left the folder. A deletion that
///   writes nothing leaves no answer to that question, and there is no other place holding it.
///
/// Contents that history already holds cost nothing to keep again: identical contents are one
/// object either way. Nothing written here is part of the history anyone reads.
fn keep_before_deleting(root: &Path, target: &Path) -> Result<(), String> {
    let prepared = match crate::git_engine::prepare(root) {
        Ok(p) => p,
        // Without a repository there is nowhere to keep anything. Deleting still works; this
        // is a safety net, not a precondition.
        Err(e) => {
            log::warn!(
                "keep_before_deleting: no repository available: {}",
                e.message()
            );
            return Ok(());
        }
    };

    let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for (on_disk, in_repo) in files_under(&prepared, root, target) {
        match std::fs::read(&on_disk) {
            Ok(content) => entries.push((in_repo, content)),
            Err(e) => {
                // A file that cannot be read cannot be kept. Say so rather than deleting it
                // while reporting success.
                return Err(format!("'{}' could not be read: {e}", on_disk.display()));
            }
        }
    }
    if entries.is_empty() {
        return Ok(());
    }

    let (author, committer) = crate::git_engine::commit_identities(prepared.repo())
        .map_err(|e| e.message().to_string())?;
    crate::git_engine::commit_paths(
        prepared.repo(),
        crate::git_engine::SNAPSHOT_REF,
        &entries,
        "keep deleted content",
        &author,
        &committer,
        // Deleting the same contents twice is two events, not one. Skipping the second would
        // leave the list showing the first deletion's moment for something deleted later.
        crate::git_engine::WhenUnchanged::Record,
    )
    .map_err(|e| e.message().to_string())?;
    log::info!("keep_before_deleting: kept {} file(s)", entries.len());
    Ok(())
}

#[tauri::command]
pub fn delete_node(root: String, path: String) -> Result<(), String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    // Capture provenance before the move (canonicalize needs the path to still exist).
    let original_rel = rel_to_root(root_p, target)?;
    // The state on disk is kept first: once the delete goes through, there is nowhere else for
    // it to come back from.
    keep_before_deleting(root_p, target)?;
    let is_dir = target.is_dir();
    // Move first (fs_ops validates is_within / root / .textree). Manifest after — a mid-crash
    // leaves an "unknown-origin" trash file (recoverable) rather than a dangling manifest entry.
    let dest = crate::fs_ops::delete_to_trash(root_p, target).map_err(|e| e.to_string())?;
    log::info!("delete_node: {}", path);
    let trash_name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "cannot read trashed name".to_string())?
        .to_string();
    let mut items = read_trash_manifest(root_p);
    items.push(TrashItem { trash_name, original_rel, deleted_at: now_secs(), is_dir });
    // NOTE: if write_trash_manifest fails here, the node is already in trash but this command
    // returns Err. The node is not lost — it surfaces as an unknown-origin orphan in list_trash
    // and can be restored. This window is data-safe by design; no behavior change intended.
    write_trash_manifest(root_p, &items)
}

/// Re-validates a vault-relative path from the (user-editable) manifest: every component must be
/// Normal and a valid name. "Security at the boundary" — the manifest is not trusted input.
fn validate_vault_rel(rel: &str) -> Result<(), String> {
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
            validate_vault_rel(&rel)?; // boundary recheck on untrusted manifest
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

/// Carries whatever was recorded along a set of moves, in every reference that holds it.
///
/// The pairs are vault-relative and applied in order, so an operation that is several moves on
/// disk stays one revision here.
///
/// Best-effort by design: the files have already moved by the time this runs, and a history
/// that could not keep up is not a reason to report the move as having failed. It is reported
/// in the log instead.
///
/// No repository is created here. A folder nothing was ever recorded in has no history for a
/// move to affect, and moving a file around is not the moment to start one.
fn record_moves(root: &Path, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    let prepared = match crate::git_engine::open_existing(root) {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(e) => {
            log::warn!("record_moves: cannot open the repository: {}", e.message());
            return;
        }
    };
    let repo = prepared.repo();
    let moves: Vec<(PathBuf, PathBuf)> = pairs
        .iter()
        .map(|(from, to)| {
            (
                prepared.path_in_repo(Path::new(from)),
                prepared.path_in_repo(Path::new(to)),
            )
        })
        .collect();
    let Ok((author, committer)) = crate::git_engine::commit_identities(repo) else {
        log::warn!("record_moves: no identity available");
        return;
    };
    // Both references are carried over. What was kept when a note was deleted describes the
    // same note, so leaving it at the old name would make a note that came back and was then
    // renamed look deleted all over again.
    for reference in [crate::git_engine::NOTES_REF, crate::git_engine::SNAPSHOT_REF] {
        match crate::git_engine::move_recorded(repo, reference, &moves, &author, &committer) {
            Ok(Some(_)) => log::info!("record_moves: {} pair(s) in {reference}", pairs.len()),
            Ok(None) => {}
            Err(e) => log::warn!("record_moves: {reference} not updated: {}", e.message()),
        }
    }
}

#[tauri::command]
pub fn rename_node(root: String, path: String, name: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    // Read before the move: resolving a path needs it to still be where it is. A folder also
    // carries a note named after it, and that note is renamed too — two moves, one revision.
    let from_rel = rel_to_root(root_p, target).ok();
    let folder_note = target
        .is_dir()
        .then(|| target.file_name().and_then(|s| s.to_str()).map(str::to_string))
        .flatten();

    let p = crate::fs_ops::rename_node(root_p, target, &name).map_err(|e| e.to_string())?;
    log::info!("rename_node: {} -> {}", path, name);

    if let (Some(from), Ok(to)) = (from_rel, rel_to_root(root_p, &p)) {
        let mut pairs = vec![(from, to.clone())];
        if let Some(old_name) = folder_note {
            pairs.push((format!("{to}/{old_name}.md"), format!("{to}/{name}.md")));
        }
        record_moves(root_p, &pairs);
    }
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn move_node(root: String, path: String, dest: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    let from_rel = rel_to_root(root_p, Path::new(&path)).ok();
    let p = crate::fs_ops::move_node(root_p, Path::new(&path), Path::new(&dest))
        .map_err(|e| e.to_string())?;
    log::info!("move_node: {} -> {}", path, dest);
    if let (Some(from), Ok(to)) = (from_rel, rel_to_root(root_p, &p)) {
        record_moves(root_p, &[(from, to)]);
    }
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn adopt_node(root: String, path: String, leaf: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    let leaf_p = Path::new(&leaf);
    let from_rel = rel_to_root(root_p, Path::new(&path)).ok();
    // The leaf is resolved before it is promoted, because promoting replaces the file it names.
    let leaf_before = rel_to_root(root_p, leaf_p).ok();

    let p = crate::fs_ops::adopt_into_leaf(root_p, Path::new(&path), leaf_p)
        .map_err(|e| e.to_string())?;

    // Adopting is a promotion followed by a move, and both have to be carried over.
    let mut pairs = Vec::new();
    if let (Some(before), Some(after)) =
        (leaf_before, p.parent().and_then(|d| folder_note_rel(root_p, d)))
    {
        pairs.push((before, after));
    }
    if let (Some(from), Ok(to)) = (from_rel, rel_to_root(root_p, &p)) {
        pairs.push((from, to));
    }
    record_moves(root_p, &pairs);
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
/// or exe) — a `.js` path is run via `node`. Production: the bundled canopy sidecar (`node` +
/// `cli.js`) under `<resource>/canopy/`.
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
    if let Some(inv) = crate::publish::canopy_from_resource_dir(&resource) {
        return Ok(inv);
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
    let result = crate::publish::run_publish(&vault, &out, &options, &canopy, crate::publish::RENDER_TIMEOUT)?;
    log::info!("publish_site: {} ({} pages)", result.out_dir, result.page_count);
    Ok(result)
}

/// Publishes the open vault to the cloud (pub.textree.me): renders locally via canopy, zips the
/// output, and uploads it to api.textree.me/publish using the stored publish token. Read-only over
/// the source (D13). Errors if no publish token is set (add one in Settings).
#[tauri::command]
pub fn publish_to_cloud(
    app: AppHandle,
    vault_path: String,
    options: crate::publish::PublishOptions,
) -> Result<crate::cloud_publish::PublishToCloudResult, String> {
    let vault = PathBuf::from(&vault_path);
    let canopy = resolve_canopy(&app)?;
    let token = crate::publish_secret::get_token()
        .ok_or("no publish token is set — add one in Settings")?;
    let result = crate::cloud_publish::publish_to_cloud(&vault, &options, &canopy, &token)?;
    log::info!("publish_to_cloud: {} ({} pages)", result.url, result.page_count);
    Ok(result)
}

/// Opens the OS app log directory in the system file explorer. Useful for diagnostic sharing.
/// Creates the directory if it does not yet exist (e.g. before the first app run that writes a log).
#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    log::info!("open_log_dir: {}", dir.display());
    tauri_plugin_opener::open_path(dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Records the given notes as one revision in the vault's history.
///
/// Only the paths named here are written. What the person working in the repository has
/// staged, which branch they have checked out, and what is in their working tree are all left
/// exactly as they were.
///
/// Returns `None` when the notes are already in the state history holds: nothing was added, and
/// saying so is the honest answer. Handing back the previous revision instead would read as a
/// new one that the history it belongs to does not list.
///
/// Refused rather than half-done when: the repository is in the middle of another operation,
/// a path lies outside the vault, or a path is covered by an ignore rule (in which case the
/// history would silently not contain what was asked for).
#[tauri::command]
pub fn commit_notes(
    root: String,
    paths: Vec<String>,
    message: String,
) -> Result<Option<String>, String> {
    let root_p = Path::new(&root);
    if paths.is_empty() {
        return Err("nothing was selected".into());
    }

    let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
    let repo = prepared.repo();

    if let Some(state) = crate::git_engine::operation_in_progress(repo) {
        log::warn!("commit_notes: refused, repository is busy: {state:?}");
        return Err(format!(
            "the repository is in the middle of another operation ({state:?})"
        ));
    }

    let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(paths.len());
    for raw in &paths {
        let target = Path::new(raw);
        if !is_within(root_p, target) {
            log::warn!("commit_notes: rejected unsafe path: {}", target.display());
            return Err("path is outside the vault".into());
        }
        let rel = rel_to_root(root_p, target)?;
        if target.is_dir() {
            // Reading a directory fails with an operating-system error that says nothing about
            // what went wrong. The caller expands a selection into files before asking.
            return Err(format!("'{rel}' is a folder, not a note"));
        }
        let in_repo = prepared.path_in_repo(Path::new(&rel));
        if repo.is_path_ignored(&in_repo).unwrap_or(false) {
            log::warn!("commit_notes: refused, path is ignored: {rel}");
            return Err(format!(
                "'{rel}' is covered by an ignore rule, so recording it would leave it out"
            ));
        }
        let content = std::fs::read(target).map_err(|e| e.to_string())?;
        entries.push((in_repo, content));
    }

    let (author, committer) =
        crate::git_engine::commit_identities(repo).map_err(|e| e.message().to_string())?;
    let oid = crate::git_engine::commit_paths(
        repo,
        crate::git_engine::NOTES_REF,
        &entries,
        &message,
        &author,
        &committer,
        crate::git_engine::WhenUnchanged::Skip,
    )
    .map_err(|e| e.message().to_string())?;

    match oid {
        Some(id) => {
            log::info!("commit_notes: {} path(s) as {}", entries.len(), id);
            Ok(Some(id.to_string()))
        }
        None => {
            log::info!("commit_notes: {} path(s) already held", entries.len());
            Ok(None)
        }
    }
}

/// One recorded state of a note, as the interface presents it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    /// Opaque handle; hand it back to `note_version_text`.
    pub id: String,
    pub message: String,
    /// Unix epoch seconds.
    pub seconds: i64,
    pub author: String,
}

/// A note that history holds but the folder no longer does.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeletedNote {
    /// Vault-root-relative, `/`-separated.
    pub rel: String,
    /// Unix epoch seconds of when it left the folder.
    ///
    /// Taken from what was kept on the way out, which is written at that moment. For a note
    /// that left some other way — removed outside the application, or before its contents were
    /// being kept — nothing recorded the moment, and the last time it was written to history
    /// stands in as the closest thing known.
    pub seconds: i64,
    /// Whether it was ever recorded deliberately, as opposed to only kept when deleted.
    pub recorded: bool,
}

/// Where a restored note's contents came from.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoredNote {
    /// Vault-root-relative, `/`-separated path the note actually landed at.
    pub rel: String,
    /// True when the contents are the ones the note held at the moment it left the folder,
    /// false when they come from the last state recorded before that.
    ///
    /// The two differ whenever a note was edited without being recorded again, which is the
    /// case where saying which one came back matters most.
    pub as_deleted: bool,
}

/// Every recorded state of one note, newest first.
#[tauri::command]
pub fn note_versions(root: String, path: String) -> Result<Vec<NoteVersion>, String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    if !is_within(root_p, target) {
        return Err("path is outside the vault".into());
    }
    let rel = rel_to_root(root_p, target)?;
    let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
    let in_repo = prepared.path_in_repo(Path::new(&rel));
    let found =
        crate::git_engine::history(prepared.repo(), crate::git_engine::NOTES_REF, &in_repo)
            .map_err(|e| e.message().to_string())?;
    Ok(found
        .into_iter()
        .map(|v| NoteVersion {
            id: v.id,
            message: v.message,
            seconds: v.seconds,
            author: v.author,
        })
        .collect())
}

/// What a note held at one recorded state. Reads objects only — the file on disk is not touched.
#[tauri::command]
pub fn note_version_text(root: String, path: String, id: String) -> Result<String, String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    if !is_within(root_p, target) {
        return Err("path is outside the vault".into());
    }
    let rel = rel_to_root(root_p, target)?;
    let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
    let in_repo = prepared.path_in_repo(Path::new(&rel));
    let content = crate::git_engine::content_at_version(
        prepared.repo(),
        crate::git_engine::NOTES_REF,
        &in_repo,
        &id,
    )
    .map_err(|e| e.message().to_string())?
    .ok_or_else(|| format!("'{rel}' is not part of that recorded state"))?;
    String::from_utf8(content).map_err(|_| "this state is not text".to_string())
}

/// Everything history holds that is no longer in the folder.
///
/// Two things end up here and the difference does not matter to whoever is looking for what
/// they deleted: notes that were recorded and later removed, and notes that were never recorded
/// but whose contents were kept when they were deleted.
#[tauri::command]
pub fn deleted_notes(root: String) -> Result<Vec<DeletedNote>, String> {
    let root_p = Path::new(&root);
    let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
    let repo = prepared.repo();

    let kept_times = crate::git_engine::last_changed(repo, crate::git_engine::SNAPSHOT_REF)
        .map_err(|e| e.message().to_string())?;
    let recorded_times = crate::git_engine::last_changed(repo, crate::git_engine::NOTES_REF)
        .map_err(|e| e.message().to_string())?;
    let recorded_paths: std::collections::HashSet<String> =
        crate::git_engine::tip_paths(repo, crate::git_engine::NOTES_REF)
            .map_err(|e| e.message().to_string())?
            .into_iter()
            .collect();

    let mut seen: std::collections::HashMap<String, DeletedNote> = std::collections::HashMap::new();
    for in_repo in recorded_paths.iter().cloned().chain(
        crate::git_engine::tip_paths(repo, crate::git_engine::SNAPSHOT_REF)
            .map_err(|e| e.message().to_string())?,
    ) {
        let Some(rel) = prepared.path_in_vault(Path::new(&in_repo)) else {
            // Something the enclosing repository holds outside this folder.
            continue;
        };
        if root_p.join(&rel).exists() {
            continue;
        }
        let rel = rel.to_string_lossy().replace('\\', "/");
        // When it left, not when it was last saved. Only the first of those answers the
        // question the list is sorted by.
        let seconds = kept_times
            .get(&in_repo)
            .or_else(|| recorded_times.get(&in_repo))
            .copied()
            .unwrap_or(0);
        seen.insert(
            rel.clone(),
            DeletedNote { rel, seconds, recorded: recorded_paths.contains(&in_repo) },
        );
    }

    let mut out: Vec<DeletedNote> = seen.into_values().collect();
    // Newest first, then by name so equal timestamps do not shuffle between calls.
    out.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.rel.cmp(&b.rel)));
    Ok(out)
}

/// When `reference` last wrote `rel`, in seconds since the epoch.
fn last_written(
    repo: &git2::Repository,
    reference: &str,
    in_repo: &Path,
) -> Result<Option<i64>, String> {
    Ok(crate::git_engine::history(repo, reference, in_repo)
        .map_err(|e| e.message().to_string())?
        .first()
        .map(|v| v.seconds))
}

/// Brings a deleted note back into the folder.
///
/// Two places may hold it — what was kept when it left, and the last state recorded before
/// that — and **the newer of the two is the one brought back**. Preferring either place by rule
/// gets it wrong in one direction or the other: reading history first loses edits that were
/// never recorded, and reading what was kept first hands back a stale copy to a note that was
/// deleted, restored, saved, and then removed some other way.
///
/// An existing file of the same name is never overwritten: the restored copy is numbered
/// alongside it, and the path it actually landed at is returned so the caller can say where.
#[tauri::command]
pub fn restore_deleted(root: String, rel: String) -> Result<RestoredNote, String> {
    let root_p = Path::new(&root);
    validate_vault_rel(&rel)?;
    let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
    let repo = prepared.repo();
    let in_repo = prepared.path_in_repo(Path::new(&rel));

    let kept = last_written(repo, crate::git_engine::SNAPSHOT_REF, &in_repo)?;
    let recorded = last_written(repo, crate::git_engine::NOTES_REF, &in_repo)?;
    // A tie goes to what was kept: it is written as the note leaves, so it is at or after
    // whatever was recorded in the same second.
    let as_deleted = match (kept, recorded) {
        (Some(k), Some(r)) => k >= r,
        (Some(_), None) => true,
        _ => false,
    };
    let reference = if as_deleted {
        crate::git_engine::SNAPSHOT_REF
    } else {
        crate::git_engine::NOTES_REF
    };

    let content = crate::git_engine::content_at_tip(repo, reference, &in_repo)
        .map_err(|e| e.message().to_string())?
        .ok_or_else(|| format!("'{rel}' is not in this folder's history"))?;

    let dest = crate::fs_ops::place_restored(root_p, &rel).map_err(|e| e.to_string())?;
    atomic_write_bytes(root_p, &dest, &content).map_err(|e| e.to_string())?;
    log::info!("restore_deleted: {} (as deleted: {as_deleted})", dest.display());
    Ok(RestoredNote { rel: rel_to_root(root_p, &dest)?, as_deleted })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn seed_note(root: &Path, rel: &str, body: &str) -> String {
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&target, body).unwrap();
        target.to_string_lossy().to_string()
    }

    /// Records `rel` under `root` and returns its absolute path.
    fn record_note(root: &Path, rel: &str, body: &str) -> String {
        let path = seed_note(root, rel, body);
        commit_notes(
            root.to_string_lossy().to_string(),
            vec![path.clone()],
            format!("recorded {rel}"),
        )
        .unwrap();
        path
    }

    #[test]
    fn a_notes_recorded_states_come_back_in_order_without_touching_the_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();

        let a = record_note(root, "sub/a.md", "first");
        std::fs::write(&a, "second").unwrap();
        commit_notes(
            root.to_string_lossy().to_string(),
            vec![a.clone()],
            "second".into(),
        )
        .unwrap();
        // A different note's revision must not show up in this note's list.
        record_note(root, "b.md", "unrelated");

        let versions = note_versions(root.to_string_lossy().to_string(), a.clone()).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].message, "second");

        let earlier = note_version_text(
            root.to_string_lossy().to_string(),
            a.clone(),
            versions[1].id.clone(),
        )
        .unwrap();
        assert_eq!(earlier, "first");
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "second",
            "reading an earlier state must leave the file as it is"
        );
    }

    #[test]
    fn deleted_notes_gathers_both_the_recorded_and_the_never_recorded() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let recorded = record_note(root, "kept.md", "recorded body");
        let never = seed_note(root, "sub/scratch.md", "never recorded");
        record_note(root, "staying.md", "still here");

        delete_node(root_s.clone(), recorded).unwrap();
        delete_node(root_s.clone(), never).unwrap();

        let deleted = deleted_notes(root_s.clone()).unwrap();
        let names: Vec<&str> = deleted.iter().map(|d| d.rel.as_str()).collect();
        assert!(names.contains(&"kept.md"), "a recorded note that was deleted");
        assert!(
            names.contains(&"sub/scratch.md"),
            "and one that was only ever kept when it went"
        );
        assert!(
            !names.contains(&"staying.md"),
            "a note still in the folder is not deleted"
        );
        assert!(deleted.iter().find(|d| d.rel == "kept.md").unwrap().recorded);
        assert!(!deleted
            .iter()
            .find(|d| d.rel == "sub/scratch.md")
            .unwrap()
            .recorded);

        // Trash holds the moved files, so nothing above depended on them still being in place.
        assert!(!root.join("kept.md").exists());
    }

    #[test]
    fn restoring_brings_a_note_back_and_takes_it_off_the_list() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let never = seed_note(root, "sub/scratch.md", "never recorded");
        delete_node(root_s.clone(), never).unwrap();

        let landed = restore_deleted(root_s.clone(), "sub/scratch.md".into()).unwrap();
        assert_eq!(landed.rel, "sub/scratch.md");
        assert_eq!(
            std::fs::read_to_string(root.join("sub").join("scratch.md")).unwrap(),
            "never recorded"
        );
        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "what is back in the folder is no longer missing from it"
        );
    }

    #[test]
    fn renaming_a_recorded_note_does_not_make_it_look_deleted() {
        // Nothing was deleted, so nothing belongs in the deleted list. Offering the old name
        // back would put a second, older copy of a live note into the folder.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "before.md", "body");
        rename_node(root_s.clone(), note, "after".into()).unwrap();

        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "a note that was renamed is still in the folder"
        );
        assert!(root.join("after.md").exists());
    }

    #[test]
    fn a_renamed_notes_earlier_states_are_still_its_own() {
        // Following the move is the other half of writing it: the states from before the
        // rename were recorded by the same person about the same note.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "before.md", "first");
        std::fs::write(&note, "second").unwrap();
        commit_notes(root_s.clone(), vec![note.clone()], "second".into()).unwrap();
        let after = rename_node(root_s.clone(), note, "after".into()).unwrap();

        let versions = note_versions(root_s.clone(), after.clone()).unwrap();
        assert_eq!(
            versions.len(),
            2,
            "the rename is followed, and is not itself one of the states"
        );
        assert_eq!(versions[0].message, "second");
        assert_eq!(
            note_version_text(root_s, after, versions[1].id.clone()).unwrap(),
            "first",
            "the earliest state is readable under the new name"
        );
    }

    #[test]
    fn moving_a_folder_carries_every_recorded_note_in_it() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        record_note(root, "src/a.md", "a");
        record_note(root, "src/deep/b.md", "b");
        std::fs::create_dir_all(root.join("dest")).unwrap();

        move_node(
            root_s.clone(),
            root.join("src").to_string_lossy().to_string(),
            root.join("dest").to_string_lossy().to_string(),
        )
        .unwrap();

        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "moving a folder deletes none of the notes in it"
        );
        assert_eq!(
            note_versions(
                root_s,
                root.join("dest").join("src").join("deep").join("b.md")
                    .to_string_lossy()
                    .to_string(),
            )
            .unwrap()
            .len(),
            1,
            "a note one level down keeps its history too"
        );
    }

    #[test]
    fn renaming_a_folder_carries_the_note_named_after_it() {
        // A folder rename is two moves on disk: the folder, and the note that carries its
        // name. Following only the first leaves the second recorded under a name nothing is at.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        record_note(root, "topic/topic.md", "the folder note");
        record_note(root, "topic/child.md", "a child");

        rename_node(
            root_s.clone(),
            root.join("topic").to_string_lossy().to_string(),
            "subject".into(),
        )
        .unwrap();

        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "renaming a folder deletes neither it nor anything in it"
        );
        assert_eq!(
            note_versions(
                root_s,
                root.join("subject").join("subject.md").to_string_lossy().to_string(),
            )
            .unwrap()
            .len(),
            1,
            "the note the folder is named after keeps its history under the new name"
        );
    }

    #[test]
    fn turning_a_note_into_a_folder_carries_its_history_down_with_it() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "topic.md", "body");
        promote_node(root_s.clone(), note).unwrap();

        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "the note is still there, one level down"
        );
        assert_eq!(
            note_versions(
                root_s,
                root.join("topic").join("topic.md").to_string_lossy().to_string(),
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn adopting_carries_both_the_promoted_note_and_the_one_taken_in() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let host = record_note(root, "host.md", "the host");
        let guest = record_note(root, "guest.md", "the guest");

        adopt_node(root_s.clone(), guest, host).unwrap();

        assert!(
            deleted_notes(root_s.clone()).unwrap().is_empty(),
            "adopting is two moves, and neither of them is a deletion"
        );
        for landed in ["host.md", "guest.md"] {
            assert_eq!(
                note_versions(
                    root_s.clone(),
                    root.join("host").join(landed).to_string_lossy().to_string(),
                )
                .unwrap()
                .len(),
                1,
                "{landed} keeps its history"
            );
        }
    }

    #[test]
    fn renaming_a_note_that_was_never_recorded_starts_no_history() {
        // There is nothing to carry over, and moving a file is not the moment to begin
        // recording one.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let root_s = root.to_string_lossy().to_string();

        let note = seed_note(root, "scratch.md", "body");
        rename_node(root_s, note, "renamed".into()).unwrap();

        assert!(
            !root.join(".git").exists(),
            "a folder nothing was recorded in stays a plain folder"
        );
    }

    #[test]
    fn restoring_brings_back_edits_that_were_never_recorded() {
        // The note was saved once, worked on afterwards without being saved again, then
        // deleted. Only the second state answers "give me back what I deleted"; handing over
        // the recorded one throws away the work and says nothing about having done so.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "a.md", "as recorded");
        std::fs::write(&note, "worked on since, never recorded").unwrap();
        delete_node(root_s.clone(), note).unwrap();

        let landed = restore_deleted(root_s.clone(), "a.md".into()).unwrap();
        assert!(landed.as_deleted, "it came back as it was when it went");
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "worked on since, never recorded"
        );
    }

    #[test]
    fn restoring_a_note_removed_outside_the_application_uses_what_was_recorded() {
        // Nothing was kept on the way out, because the application was not the one that took
        // it. History is all there is, and the answer says so.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "a.md", "as recorded");
        std::fs::remove_file(&note).unwrap();

        let landed = restore_deleted(root_s.clone(), "a.md".into()).unwrap();
        assert!(!landed.as_deleted, "there was no kept state to prefer");
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "as recorded"
        );
    }

    #[test]
    fn restoring_prefers_a_version_saved_after_the_note_was_last_kept() {
        // Deleted, brought back, saved, then removed some other way. What was kept is now the
        // older of the two, and preferring it by rule would hand back a stale copy.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = seed_note(root, "a.md", "first pass");
        delete_node(root_s.clone(), note).unwrap();
        restore_deleted(root_s.clone(), "a.md".into()).unwrap();

        // A revision has to land in a later second than the kept one for "newer" to mean
        // anything at all — timestamps here have one-second resolution.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let note = record_note(root, "a.md", "second pass, recorded");
        std::fs::remove_file(&note).unwrap();

        let landed = restore_deleted(root_s.clone(), "a.md".into()).unwrap();
        assert!(!landed.as_deleted);
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "second pass, recorded"
        );
    }

    #[test]
    fn the_deleted_list_is_ordered_by_when_notes_left_the_folder() {
        // An old note deleted just now belongs above a recent one deleted before it. Ordering
        // by when each was last saved gets that backwards.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let long_ago = record_note(root, "long-ago.md", "written first");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let recent = record_note(root, "recent.md", "written later");

        // The one saved first is the one deleted last.
        delete_node(root_s.clone(), recent).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        delete_node(root_s.clone(), long_ago).unwrap();

        let deleted = deleted_notes(root_s.clone()).unwrap();
        let names: Vec<&str> = deleted.iter().map(|d| d.rel.as_str()).collect();
        assert_eq!(
            names,
            vec!["long-ago.md", "recent.md"],
            "newest first means most recently deleted, not most recently saved"
        );
    }

    #[test]
    fn recording_a_note_that_has_not_changed_reports_that_nothing_was_added() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "a.md", "unchanged");
        let again = commit_notes(root_s.clone(), vec![note.clone()], "again".into()).unwrap();

        assert!(
            again.is_none(),
            "nothing was written, so there is no revision to point at"
        );
        assert_eq!(
            note_versions(root_s, note).unwrap().len(),
            1,
            "and the history it would have been part of is unchanged"
        );
    }

    #[test]
    fn restoring_over_an_existing_name_keeps_both() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = seed_note(root, "a.md", "the deleted one");
        delete_node(root_s.clone(), note).unwrap();
        seed_note(root, "a.md", "a different note with the same name");

        let landed = restore_deleted(root_s.clone(), "a.md".into()).unwrap();
        assert_eq!(landed.rel, "a (1).md");
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "a different note with the same name",
            "what was already there must survive"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a (1).md")).unwrap(),
            "the deleted one"
        );
    }

    #[test]
    fn restoring_refuses_a_path_that_climbs_out_of_the_vault() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        for attempt in ["../escape.md", "sub/../../escape.md", ""] {
            assert!(restore_deleted(root_s.clone(), attempt.into()).is_err());
        }
        assert!(
            !root.parent().unwrap().join("escape.md").exists(),
            "nothing may be written outside the vault"
        );
    }

    #[test]
    fn committing_records_the_named_paths_and_nothing_else() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let repo = git2::Repository::init(root).unwrap();

        // Unrelated work the person has staged. It must come through untouched.
        std::fs::write(root.join("app.txt"), "their work").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("app.txt")).unwrap();
        index.write().unwrap();
        let staged_before = index.len();

        let a = seed_note(root, "a.md", "# a");
        let b = seed_note(root, "sub/b.md", "# b");

        let oid = commit_notes(
            root.to_string_lossy().to_string(),
            vec![a, b],
            "record two notes".into(),
        )
        .unwrap();
        assert_eq!(
            oid.expect("something was written").len(),
            40,
            "a revision identifier is returned"
        );

        let tree = repo
            .find_reference(crate::git_engine::NOTES_REF)
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .tree()
            .unwrap();
        assert!(tree.get_path(Path::new("a.md")).is_ok());
        assert!(tree.get_path(&Path::new("sub").join("b.md")).is_ok());
        assert!(
            tree.get_path(Path::new("app.txt")).is_err(),
            "staged work must not be swept in"
        );

        let index_after = repo.index().unwrap();
        assert_eq!(index_after.len(), staged_before, "the index is untouched");
    }

    #[test]
    fn committing_is_refused_while_the_repository_is_busy() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let repo = git2::Repository::init(root).unwrap();
        let a = seed_note(root, "a.md", "# a");
        std::fs::write(repo.path().join("MERGE_HEAD"), "0\n").unwrap();

        let err = commit_notes(
            root.to_string_lossy().to_string(),
            vec![a],
            "record a note".into(),
        )
        .unwrap_err();

        assert!(err.contains("another operation"), "got: {err}");
        assert!(
            repo.find_reference(crate::git_engine::NOTES_REF).is_err(),
            "nothing may be recorded when the attempt is refused"
        );
    }

    #[test]
    fn committing_an_ignored_path_is_refused_rather_than_silently_dropped() {
        // Without this, the history simply would not contain what was asked for, and there
        // would be nothing on screen to explain why.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let repo = git2::Repository::init(root).unwrap();
        std::fs::write(root.join(".gitignore"), "drafts/\n").unwrap();
        let hidden = seed_note(root, "drafts/a.md", "# a");

        let err = commit_notes(
            root.to_string_lossy().to_string(),
            vec![hidden],
            "record a note".into(),
        )
        .unwrap_err();

        assert!(err.contains("ignore rule"), "got: {err}");
        assert!(repo.find_reference(crate::git_engine::NOTES_REF).is_err());
    }

    #[test]
    fn committing_a_path_outside_the_vault_is_refused() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        std::fs::create_dir_all(&root).unwrap();
        git2::Repository::init(&root).unwrap();
        let outside = seed_note(tmp.path(), "elsewhere.md", "# elsewhere");

        let err = commit_notes(
            root.to_string_lossy().to_string(),
            vec![outside],
            "record a note".into(),
        )
        .unwrap_err();
        assert!(err.contains("outside the vault"), "got: {err}");
    }

    #[test]
    fn committing_a_folder_says_so_instead_of_failing_obscurely() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();

        let err = commit_notes(
            root.to_string_lossy().to_string(),
            vec![root.join("sub").to_string_lossy().to_string()],
            "record".into(),
        )
        .unwrap_err();
        assert!(err.contains("is a folder"), "got: {err}");
    }

    #[test]
    fn committing_nothing_is_refused() {
        let tmp = TempDir::new().unwrap();
        let err = commit_notes(
            tmp.path().to_string_lossy().to_string(),
            vec![],
            "record".into(),
        )
        .unwrap_err();
        assert!(err.contains("nothing was selected"), "got: {err}");
    }

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
        let root = TempDir::new().unwrap();
        let f = root.path().join("note.md");
        std::fs::write(&f, "old").unwrap();
        atomic_write(root.path(), &f, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_creates_when_absent() {
        let root = TempDir::new().unwrap();
        let f = root.path().join("fresh.md");
        atomic_write(root.path(), &f, "hi").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "hi");
    }

    #[test]
    fn temp_dir_lives_in_repository_storage() {
        let root = TempDir::new().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        assert_eq!(temp_dir(root.path()), repo.path().join(TEMP_DIR_NAME));
    }

    #[test]
    fn temp_dir_of_a_nested_folder_uses_the_governing_repository() {
        // The folder someone opens may sit inside a repository they are working in. Staging
        // under that folder would put temp files in their working tree; the repository's own
        // storage is outside it.
        let root = TempDir::new().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        let inner = root.path().join("docs");
        std::fs::create_dir_all(&inner).unwrap();

        assert_eq!(temp_dir(&inner), repo.path().join(TEMP_DIR_NAME));
        assert!(!temp_dir(&inner).starts_with(&inner));
    }

    #[test]
    fn temp_dir_falls_back_when_no_repository_governs_the_folder() {
        // Writing must keep working before a repository exists.
        let root = Path::new("/nowhere-that-exists");
        assert_eq!(temp_dir(root), root.join(".textree").join("tmp"));
    }

    #[test]
    fn atomic_write_leaves_no_temp_beside_the_target() {
        // Temp files must not litter the user's content folders (sync tools churn on them),
        // and must not appear as changes in a repository they are working in.
        let root = TempDir::new().unwrap();
        git2::Repository::init(root.path()).unwrap();
        let notes = root.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        let f = notes.join("foo.md");
        atomic_write(root.path(), &f, "body").unwrap();

        assert_eq!(std::fs::read_to_string(&f).unwrap(), "body");
        let entries: Vec<_> = std::fs::read_dir(&notes).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1, "only foo.md, no temp litter");
        assert!(temp_dir(root.path()).is_dir());
        assert!(
            !root.path().join(".textree").exists(),
            "nothing of ours is left in the folder itself"
        );
    }

    #[test]
    fn clear_temp_dir_also_sweeps_the_older_location() {
        // Upgrading must not leave orphans behind at the previous address.
        let root = TempDir::new().unwrap();
        git2::Repository::init(root.path()).unwrap();
        let legacy = root.path().join(".textree").join("tmp");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join(".tmpOrphan"), "stale").unwrap();

        clear_temp_dir(root.path());
        assert_eq!(std::fs::read_dir(&legacy).unwrap().flatten().count(), 0);
    }

    #[test]
    fn clear_temp_dir_removes_orphaned_temps() {
        // A crash mid-write can orphan a temp; it would otherwise sync forever. Cleared on open.
        let root = TempDir::new().unwrap();
        let tmp = temp_dir(root.path());
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(".tmpOrphan"), "stale").unwrap();
        clear_temp_dir(root.path());
        assert_eq!(std::fs::read_dir(&tmp).unwrap().flatten().count(), 0);
    }

    #[test]
    fn clear_temp_dir_is_graceful_when_absent() {
        // No temp dir yet (fresh vault) → no error, no panic.
        let root = TempDir::new().unwrap();
        clear_temp_dir(root.path());
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
        // The escape destination must not exist — confirm no file was written outside the vault.
        assert!(
            !tmp.path().parent().unwrap().join("escape.md").exists(),
            "the escaped file must not be created outside the vault"
        );
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

    /// Covers the `is_dir=true` restore branch: delete a folder-note directory, confirm the manifest
    /// records it as a directory, then restore it and verify the directory and its folder note are back.
    #[test]
    fn restore_node_folder_note_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_string_lossy().to_string();

        // Create a folder-note structure: journal/journal.md
        let journal_dir = tmp.path().join("journal");
        std::fs::create_dir(&journal_dir).unwrap();
        std::fs::write(journal_dir.join("journal.md"), "daily notes").unwrap();

        // Delete the whole journal/ directory.
        delete_node(root.clone(), journal_dir.to_string_lossy().into()).unwrap();
        assert!(!journal_dir.exists(), "directory removed from vault after delete");

        // Manifest must record it as a directory with the correct original_rel.
        let items = read_trash_manifest(tmp.path());
        assert_eq!(items.len(), 1);
        assert!(items[0].is_dir, "manifest entry must be marked as a directory");
        assert_eq!(items[0].original_rel, "journal");
        let trash_name = items[0].trash_name.clone();

        // Restore: the directory and its folder note should reappear at the original location.
        let restored_rel = restore_node(root.clone(), trash_name).unwrap();
        assert_eq!(restored_rel, "journal", "restored to original vault-relative path");
        assert!(journal_dir.is_dir(), "journal/ directory is back");
        assert!(
            journal_dir.join("journal.md").is_file(),
            "the folder note journal/journal.md is restored inside the directory"
        );
        assert_eq!(
            std::fs::read_to_string(journal_dir.join("journal.md")).unwrap(),
            "daily notes",
            "folder note content is preserved"
        );
        assert!(read_trash_manifest(tmp.path()).is_empty(), "manifest entry removed after restore");
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

#[cfg(test)]
mod onboarding_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn seeds_welcome_in_fresh_vault() {
        let base = tempdir().unwrap();
        let vault = ensure_vault_at(base.path()).unwrap();
        assert_eq!(vault, base.path().join("Textree"));
        let welcome = vault.join("welcome.md");
        assert!(welcome.is_file(), "welcome.md should be seeded in a fresh vault");
        let body = std::fs::read_to_string(&welcome).unwrap();
        assert!(body.contains("Welcome to Textree"));
    }

    #[test]
    fn does_not_seed_when_markdown_already_exists() {
        let base = tempdir().unwrap();
        let vault_dir = base.path().join("Textree");
        std::fs::create_dir_all(&vault_dir).unwrap();
        std::fs::write(vault_dir.join("note.md"), "my own note").unwrap();
        let vault = ensure_vault_at(base.path()).unwrap();
        assert!(!vault.join("welcome.md").exists(), "must not seed into a non-empty vault");
        // existing file untouched (non-destructive)
        assert_eq!(std::fs::read_to_string(vault_dir.join("note.md")).unwrap(), "my own note");
    }

    #[test]
    fn does_not_overwrite_existing_welcome() {
        let base = tempdir().unwrap();
        let vault_dir = base.path().join("Textree");
        std::fs::create_dir_all(&vault_dir).unwrap();
        std::fs::write(vault_dir.join("welcome.md"), "user edited welcome").unwrap();
        ensure_vault_at(base.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(vault_dir.join("welcome.md")).unwrap(),
            "user edited welcome",
            "an existing welcome.md must never be overwritten"
        );
    }

    #[test]
    fn is_idempotent_on_second_call() {
        let base = tempdir().unwrap();
        ensure_vault_at(base.path()).unwrap();
        // user deletes the seed, then app restarts → must NOT re-seed (welcome was intentional, once)
        std::fs::write(base.path().join("Textree").join("keep.md"), "x").unwrap();
        std::fs::remove_file(base.path().join("Textree").join("welcome.md")).unwrap();
        ensure_vault_at(base.path()).unwrap();
        assert!(!base.path().join("Textree").join("welcome.md").exists());
    }

    /// A base path that cannot have `base/Textree` created under it — a regular file occupies a
    /// path component, so `create_dir_all` fails. Mirrors a OneDrive-redirected/uninitialized
    /// Documents dir whose returned path is invalid (the real-world trigger of os error 3).
    fn uncreatable_base() -> (tempfile::TempDir, PathBuf) {
        let holder = tempdir().unwrap();
        let file = holder.path().join("a_file");
        std::fs::write(&file, "x").unwrap();
        // `file` is a regular file, so `file/sub/Textree` can never be created.
        (holder, file.join("sub"))
    }

    #[test]
    fn no_fallback_flag_when_primary_base_works() {
        let good = tempdir().unwrap();
        let bases = vec![good.path().to_path_buf()];
        let (vault, fell_back) = first_creatable_vault(&bases).unwrap();
        assert!(!fell_back, "a working primary base must not report a fallback");
        assert_eq!(vault, good.path().join("Textree"));
        assert!(vault.join("welcome.md").is_file());
    }

    #[test]
    fn falls_back_when_primary_base_is_uncreatable() {
        let (_holder, invalid) = uncreatable_base();
        let good = tempdir().unwrap();
        // Primary (invalid) fails create_dir_all → must land on the second candidate and flag it.
        let bases = vec![invalid, good.path().to_path_buf()];
        let (vault, fell_back) = first_creatable_vault(&bases).unwrap();
        assert!(fell_back, "must report falling back away from the unusable primary base");
        assert_eq!(vault, good.path().join("Textree"));
        assert!(vault.join("welcome.md").is_file(), "the fallback vault is fully seeded");
    }

    #[test]
    fn errors_when_every_candidate_base_fails() {
        let (_holder, invalid) = uncreatable_base();
        let bases = vec![invalid];
        let err = first_creatable_vault(&bases)
            .expect_err("with no creatable base, resolution must error (never silently no-op)");
        // The os error code is preserved (not stringified away) so telemetry can carry it.
        assert!(err.1.is_some(), "the failing candidate's raw os error code must be captured");
    }
}
