use crate::host;
use crate::pathsafe::is_within;
use crate::search::{IndexHandle, IndexState, SearchHit};
use crate::self_write::SelfWrites;
use crate::vault::{self, TreeNode};
use crate::watcher::WatcherHandle;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
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

/// Atomic write for a file that is not inside the vault, so the temp file goes beside the
/// destination. A rename is only atomic within one volume, and settings live wherever the
/// person's home is — which is often not the volume the notes are on.
fn atomic_write_beside(path: &Path, content: &str) -> io::Result<()> {
    atomic_bytes_beside(path, content.as_bytes())
}

/// The same guarantee for content that is not necessarily text, so that carrying a file across
/// does not require being able to read it.
fn atomic_bytes_beside(path: &Path, content: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
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

/// A short, stable label for a folder, used to give each one its own place to keep settings.
///
/// The readable part is there so the directory can be recognised by eye; the digest is what
/// makes it unambiguous when two folders end with the same name.
fn folder_key(root: &Path) -> String {
    let resolved = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in resolved.to_string_lossy().to_lowercase().bytes() {
        digest ^= u64::from(byte);
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let readable: String = resolved
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(24)
        .collect();
    if readable.is_empty() {
        format!("{digest:016x}")
    } else {
        format!("{readable}-{digest:016x}")
    }
}

/// Where settings that belong to the person rather than to the notes are kept.
///
/// Outside the notes folder on purpose: a preference is not a note, it is not something anyone
/// wants to see in their folder, and it should not travel when the notes do. Keyed per folder,
/// because "which notes are favourites" only means anything about one set of notes.
fn personal_dir(root: &Path) -> Result<PathBuf, String> {
    let base = std::env::var_os("TEXTREE_PERSONAL_BASE")
        .map(PathBuf::from)
        .or_else(personal_base)
        .ok_or_else(|| "cannot find a place to keep settings".to_string())?;
    Ok(base.join(".textree").join("vaults").join(folder_key(root)))
}

#[cfg(not(test))]
fn personal_base() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Under test, somewhere of this run's own — writing settings must never reach the home
/// directory of whoever is running the suite.
#[cfg(test)]
fn personal_base() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static ELSEWHERE: OnceLock<tempfile::TempDir> = OnceLock::new();
    Some(
        ELSEWHERE
            .get_or_init(|| tempfile::TempDir::new().expect("a place to keep test settings"))
            .path()
            .to_path_buf(),
    )
}

/// Builds the path of one settings file. Anything other than a plain name component is rejected,
/// so nothing can address a location outside the directory it is confined to.
fn sidecar_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() {
        return Err("sidecar path is empty".into());
    }
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err("invalid settings path".into());
        }
    }
    Ok(personal_dir(root)?.join(rel_path))
}

/// The same file as it was kept before settings moved out of the notes folder.
fn legacy_sidecar_path(root: &Path, rel: &str) -> PathBuf {
    root.join(".textree").join(rel)
}

const TRASH_MANIFEST: &str = "trash.json";

/// One trashed node's provenance. Lives in `.textree/trash.json` (sidecar, regeneratable
/// in spirit: if lost, the trash files themselves remain the truth).
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
    // Stays beside the trash it describes: the two are one thing, and the whole thing is
    // carried over at once rather than half of it at a time.
    let path = legacy_sidecar_path(root, TRASH_MANIFEST);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Reads one of this folder's settings. `None` when it has never been written.
#[tauri::command]
pub fn read_sidecar(root: String, rel: String) -> Result<Option<String>, String> {
    let path = sidecar_path(Path::new(&root), &rel)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes one of this folder's settings, leaving the folder itself untouched.
#[tauri::command]
pub fn write_sidecar(root: String, rel: String, content: String) -> Result<(), String> {
    let path = sidecar_path(Path::new(&root), &rel)?;
    atomic_write_beside(&path, &content).map_err(|e| e.to_string())
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

/// What became of a save.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WriteOutcome {
    /// The note now holds the new content.
    Written,
    /// The note no longer held what the save was based on, so nothing was written. `disk` is what
    /// it holds instead, or `None` when it is gone.
    Conflict { disk: Option<String> },
}

/// Why a save would not replace the note, if it would not: the note must still hold `expected`,
/// the content the editor and disk last agreed on. Anything else means someone changed it since,
/// and writing would silently throw that change away.
///
/// The check and the rename are not one atomic step — nothing on an ordinary filesystem offers
/// that — but they are microseconds apart in one process, instead of the round trip between the
/// editor reading a note and its next save arriving.
fn stale_base(path: &Path, expected: &str) -> io::Result<Option<WriteOutcome>> {
    match std::fs::read_to_string(path) {
        Ok(disk) if disk == expected => Ok(None),
        Ok(disk) => Ok(Some(WriteOutcome::Conflict { disk: Some(disk) })),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(Some(WriteOutcome::Conflict { disk: None }))
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn write_note(
    root: String,
    path: String,
    content: String,
    expected: String,
    self_writes: State<'_, Arc<SelfWrites>>,
    index: State<'_, Arc<IndexHandle>>,
    host: State<'_, Arc<host::HostHandle>>,
) -> Result<WriteOutcome, String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    // Not `is_within`: that needs the note to exist, and a note deleted outside the app (with or
    // without its folder) must come back as "gone" below, not as an error about where it is.
    if crate::pathsafe::rel_within(&root, &path).is_none() {
        log::warn!("write_note: rejected unsafe path: {}", path.display());
        return Err("path is outside the vault".into());
    }
    if let Some(conflict) = stale_base(&path, &expected).map_err(|e| e.to_string())? {
        log::info!("write_note: {} changed since it was loaded; not written", path.display());
        return Ok(conflict);
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
            // Background semantic index. Runs without asking because it only reads: it derives a
            // search index and never alters what the person wrote.
            let host_arc = host.inner().clone();
            let (v, p) = (root.to_string_lossy().to_string(), path.to_string_lossy().to_string());
            tauri::async_runtime::spawn_blocking(move || {
                host::index_note(&host_arc, &v, &p);
            });
            Ok(WriteOutcome::Written)
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

    // Background semantic reindex. Runs without asking for the same reason as the per-note
    // index above: it only reads, and changes nothing a person wrote.
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

/// Keeps the state of everything at or under a path, and records that it happened.
///
/// Everything is kept, not only what history lacks. Two reasons, and each alone is enough:
///
/// - Being recorded once says nothing about the state on disk now. A note recorded, then edited
///   without being recorded again, then deleted, has its newer state nowhere else — skipping it
///   because the path appears in history loses exactly the work that was never saved.
/// - The revision is also the only record of *when* something left the folder. A deletion that
///   writes nothing leaves no answer to that question, and there is no other place holding it.
///
/// Called before anything that replaces what is on disk, not only before deleting: going back to
/// an earlier state overwrites the current one just as finally.
///
/// Contents that history already holds cost nothing to keep again: identical contents are one
/// object either way. Nothing written here is part of the history anyone reads.
fn keep_state_of(root: &Path, target: &Path) -> Result<(), String> {
    let prepared = match crate::git_engine::prepare(root) {
        Ok(p) => p,
        // Without a repository there is nowhere to keep anything. Deleting still works; this
        // is a safety net, not a precondition.
        Err(e) => {
            log::warn!(
                "keep_state_of: no repository available: {}",
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
    log::info!("keep_state_of: kept {} file(s)", entries.len());
    Ok(())
}

/// Deletes a note or folder, once its state is somewhere it can be brought back from.
///
/// Nothing is set aside inside the notes folder. A copy there was how a deletion used to be
/// undoable; the history is that now. Keeping both would leave two places offering to bring the
/// same note back — and a folder holding something that is not a note.
///
/// The order is not an implementation detail: keeping comes first, because afterwards there is
/// nowhere left to read the state from.
#[tauri::command]
pub fn delete_node(root: String, path: String) -> Result<(), String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    if !is_within(root_p, target) {
        return Err("path is outside the vault".into());
    }
    if std::fs::canonicalize(target).ok() == std::fs::canonicalize(root_p).ok() {
        return Err("the folder itself cannot be deleted".into());
    }
    // Nothing whose name begins with a dot, at any depth. The tree never offers these, but this
    // is where paths are checked, and the cost of the check being somewhere else is total: asked
    // to delete the repository, this would read all of it into a commit, write that commit inside
    // the very directory it is about to remove, and then remove it — losing the notes, their
    // history, and the copy just made of them in one step.
    if target
        .strip_prefix(root_p)
        .unwrap_or(target)
        .components()
        .any(|c| matches!(c, Component::Normal(s) if s.to_string_lossy().starts_with('.')))
    {
        return Err("that is not a note".into());
    }
    keep_state_of(root_p, target)?;
    if target.is_dir() {
        std::fs::remove_dir_all(target).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(target).map_err(|e| e.to_string())?;
    }
    log::info!("delete_node: {}", path);
    Ok(())
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

/// Publishes the open vault to a static site by spawning canopy. Read-only over the source:
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
/// the source. Errors if no publish token is set (add one in Settings).
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

/// The repository a folder's history lives in, without making one.
///
/// `Ok(None)` is the ordinary answer for a folder nothing has been recorded in. Looking at what
/// was recorded is not recording, and it must not turn a plain folder into a repository.
fn history_repo(root: &Path) -> Result<Option<crate::git_engine::VaultRepo>, String> {
    crate::git_engine::open_existing(root).map_err(|e| e.message().to_string())
}

/// Every recorded state of one note, newest first.
///
/// The note does not have to still be in the folder: this is how a deleted one is looked at
/// before deciding whether to bring it back.
#[tauri::command]
pub fn note_versions(root: String, path: String) -> Result<Vec<NoteVersion>, String> {
    let root_p = Path::new(&root);
    let rel = crate::pathsafe::rel_within(root_p, Path::new(&path))
        .ok_or_else(|| "path is outside the vault".to_string())?;
    let Some(prepared) = history_repo(root_p)? else {
        return Ok(Vec::new());
    };
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
    let rel = crate::pathsafe::rel_within(root_p, Path::new(&path))
        .ok_or_else(|| "path is outside the vault".to_string())?;
    let prepared = history_repo(root_p)?
        .ok_or_else(|| format!("'{rel}' is not part of that recorded state"))?;
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
    let Some(prepared) = history_repo(root_p)? else {
        return Ok(Vec::new());
    };
    let repo = prepared.repo();

    let recorded_paths: std::collections::HashSet<String> =
        crate::git_engine::tip_paths(repo, crate::git_engine::NOTES_REF)
            .map_err(|e| e.message().to_string())?
            .into_iter()
            .collect();

    // Which notes are missing is settled first, from the folder alone. Only then is the history
    // asked anything, and only about those — the ones still in the folder have no question to
    // answer, and there are usually far more of them.
    let mut missing: Vec<(String, String)> = Vec::new();
    let mut present = crate::fs_ops::PresentFiles::default();
    for in_repo in recorded_paths.iter().cloned().chain(
        crate::git_engine::tip_paths(repo, crate::git_engine::SNAPSHOT_REF)
            .map_err(|e| e.message().to_string())?,
    ) {
        let Some(rel) = prepared.path_in_vault(Path::new(&in_repo)) else {
            // Something the enclosing repository holds outside this folder.
            continue;
        };
        if present.contains(&root_p.join(&rel)) {
            continue;
        }
        missing.push((in_repo, rel.to_string_lossy().replace('\\', "/")));
    }
    // Deduplicated, because a note that was recorded and then deleted is named by both references
    // and would otherwise be asked about twice. The history walk stops once it has an answer for
    // everything asked, and a list that can never be satisfied — two entries, one answer — stops
    // only at the end of the history, which is the cost this whole path exists to avoid.
    let asking: Vec<String> = missing
        .iter()
        .map(|(in_repo, _)| in_repo.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let kept_times =
        crate::git_engine::last_written(repo, crate::git_engine::SNAPSHOT_REF, &asking)
            .map_err(|e| e.message().to_string())?;
    let recorded_times =
        crate::git_engine::last_written(repo, crate::git_engine::NOTES_REF, &asking)
            .map_err(|e| e.message().to_string())?;

    let mut seen: std::collections::HashMap<String, DeletedNote> = std::collections::HashMap::new();
    for (in_repo, rel) in missing {
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

/// When `reference` last wrote one path, in seconds since the epoch.
fn last_written_at(
    repo: &git2::Repository,
    reference: &str,
    in_repo: &Path,
) -> Result<Option<i64>, String> {
    let asking = [in_repo.to_string_lossy().replace('\\', "/")];
    Ok(crate::git_engine::last_written(repo, reference, &asking)
        .map_err(|e| e.message().to_string())?
        .into_values()
        .next())
}

/// What a folder gave up when the application stopped keeping things inside it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MoveOut {
    /// Whether settings — favourites, ordering — were carried over to where they live now.
    pub settings: bool,
    /// How many notes the set-aside copies held, now reachable as deleted notes.
    pub notes: usize,
}

impl MoveOut {
    fn happened(&self) -> bool {
        self.settings || self.notes > 0
    }
}

/// Every file under `dir`, paired with its path relative to `dir`.
fn files_beneath(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if current.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
            continue;
        }
        if let Ok(rel) = current.strip_prefix(dir) {
            found.push((current.clone(), rel.to_path_buf()));
        }
    }
    found
}

/// Carries the copies an earlier version set aside into the history, dated as they were.
///
/// The date matters as much as the contents: the list of deleted notes is ordered by when each
/// one left, and the copies are the only place that answer exists for anything deleted before
/// the history started keeping it. Losing it here would silently reorder someone's list.
fn carry_over_set_aside(root: &Path, prepared: &crate::git_engine::VaultRepo) -> CarriedOver {
    let dir = root.join(".textree").join("trash");
    if !dir.exists() {
        return CarriedOver { notes: 0, everything: true };
    }
    // What is actually in there, which is not the same as what the list says is in there. A
    // corrupt list reads as empty, an entry can name a file that is gone, and a copy can be
    // sitting there with no entry at all — the old restore screen showed those as unknown
    // origin and could still put them back. Each name is struck off as it is carried, and
    // whatever is left over is why the folder stays.
    let mut left_over: BTreeSet<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    let manifest = read_trash_manifest(root);
    let repo = prepared.repo();
    let Ok((author, committer)) = crate::git_engine::commit_identities(repo) else {
        log::warn!("carry_over_set_aside: no identity available");
        return CarriedOver { notes: 0, everything: false };
    };

    // Oldest first, so the newest write for any one path is also the latest deletion of it.
    let mut items = manifest.clone();
    items.sort_by_key(|item| item.deleted_at);

    let mut carried = 0usize;
    for item in &items {
        let put_aside = dir.join(&item.trash_name);
        if !put_aside.exists() || validate_vault_rel(&item.original_rel).is_err() {
            continue;
        }
        let sources = if item.is_dir {
            files_beneath(&put_aside)
        } else {
            vec![(put_aside.clone(), PathBuf::new())]
        };
        let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for (on_disk, below) in sources {
            let Ok(content) = std::fs::read(&on_disk) else {
                continue;
            };
            let mut target = PathBuf::from(&item.original_rel);
            if !below.as_os_str().is_empty() {
                target = target.join(&below);
            }
            entries.push((prepared.path_in_repo(&target), content));
        }
        if entries.is_empty() {
            continue;
        }
        let when = git2::Time::new(item.deleted_at as i64, 0);
        let dated = |who: &git2::Signature<'_>| {
            git2::Signature::new(
                who.name().unwrap_or("Textree"),
                who.email().unwrap_or("noreply@textree.me"),
                &when,
            )
        };
        let (Ok(author_then), Ok(committer_then)) = (dated(&author), dated(&committer)) else {
            continue;
        };
        match crate::git_engine::commit_paths(
            repo,
            crate::git_engine::SNAPSHOT_REF,
            &entries,
            "keep deleted content",
            &author_then,
            &committer_then,
            crate::git_engine::WhenUnchanged::Record,
        ) {
            Ok(_) => {
                carried += entries.len();
                left_over.remove(&item.trash_name);
            }
            Err(e) => log::warn!("carry_over_set_aside: {} not carried: {}", item.original_rel, e.message()),
        }
    }
    if !left_over.is_empty() {
        log::warn!(
            "carry_over_set_aside: {} item(s) stay where they are: {}",
            left_over.len(),
            left_over.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    CarriedOver { notes: carried, everything: left_over.is_empty() }
}

/// What came out of the set-aside copies, and whether anything had to be left behind.
///
/// The second half is the one that matters: the folder is only removed once there is nothing in
/// it that exists nowhere else, and a copy that could not be carried — unreadable, unnamed by the
/// list, refused by the repository — is exactly such a thing.
struct CarriedOver {
    notes: usize,
    everything: bool,
}

/// Moves everything the application keeps out of the notes folder, keeping every file.
///
/// Run when a folder is opened, and safe to run again: a folder that has nothing left inside it
/// reports that nothing happened.
///
/// The copies set aside for deleted notes go into the history rather than being thrown away —
/// they are still the only copy of anything deleted before the history started keeping them.
/// Settings go where settings live now. Only then is the empty directory removed.
#[tauri::command]
pub fn move_state_out_of_vault(root: String) -> Result<MoveOut, String> {
    let root_p = Path::new(&root);
    let inside = root_p.join(".textree");
    if !inside.exists() {
        return Ok(MoveOut::default());
    }
    let mut moved = MoveOut::default();

    // Settings first: cheap, and it cannot fail in a way that costs anything.
    // Every settings file the application has ever kept in there. A name missing from this list
    // is a file left behind in a folder the notice claims is clean.
    for rel in ["favorites.json", "order.json", "views.json"] {
        let from = legacy_sidecar_path(root_p, rel);
        if !from.is_file() {
            continue;
        }
        let to = sidecar_path(root_p, rel)?;
        if to.exists() {
            // Already carried over on an earlier run; the copy left behind is the stale one.
            let _ = std::fs::remove_file(&from);
            continue;
        }
        // Read as bytes and carried across unchanged. Insisting on valid text here would abort
        // the whole move over one damaged file, and the abort is not where it ends: the next
        // write puts a fresh file at the new location, and the move after that sees one there
        // and removes the old one — so the settings that could not be read get discarded by the
        // recovery rather than by anything that decided to.
        let content = std::fs::read(&from).map_err(|e| e.to_string())?;
        atomic_bytes_beside(&to, &content).map_err(|e| e.to_string())?;
        std::fs::remove_file(&from).map_err(|e| e.to_string())?;
        moved.settings = true;
    }

    // The copies set aside need somewhere to go before they can be removed from the folder.
    if root_p.join(".textree").join("trash").exists() {
        let prepared = crate::git_engine::prepare(root_p).map_err(|e| e.message().to_string())?;
        let carried = carry_over_set_aside(root_p, &prepared);
        moved.notes = carried.notes;
        // Only once every copy is reachable from somewhere else. Removing the folder is the one
        // step here that cannot be taken back, and a copy that failed to carry has no other copy
        // by definition — that is what being set aside meant. Leaving the folder is untidy; the
        // alternative is destroying the only remaining copy of something someone deleted but did
        // not throw away.
        if carried.everything {
            std::fs::remove_dir_all(root_p.join(".textree").join("trash"))
                .map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(legacy_sidecar_path(root_p, TRASH_MANIFEST));
        }
    }

    // Anything else in there is the application's too — temp files it wrote, and nothing a
    // person put there — so the directory goes once it holds nothing.
    let _ = std::fs::remove_dir_all(root_p.join(LEGACY_TEMP_DIR[0]).join(LEGACY_TEMP_DIR[1]));
    let _ = std::fs::remove_dir(&inside);

    if moved.happened() {
        log::info!(
            "move_state_out_of_vault: settings={} notes={}",
            moved.settings,
            moved.notes
        );
    }
    Ok(moved)
}

/// Puts a note back to one of its recorded states.
///
/// What is on disk right now is kept first. Going back is the one action here that overwrites
/// work rather than adding to it, and work that was never recorded has nowhere else to be —
/// so the state being replaced is put where the deleted ones go, and stays reachable.
///
/// The note has to still be in the folder: bringing back one that is not is `restore_deleted`,
/// which decides for itself which state that should be.
#[tauri::command]
pub fn restore_version(root: String, path: String, id: String) -> Result<(), String> {
    let root_p = Path::new(&root);
    let target = Path::new(&path);
    if !is_within(root_p, target) {
        return Err("path is outside the vault".into());
    }
    let rel = rel_to_root(root_p, target)?;
    let prepared = history_repo(root_p)?
        .ok_or_else(|| format!("'{rel}' is not part of that recorded state"))?;
    let in_repo = prepared.path_in_repo(Path::new(&rel));
    let content = crate::git_engine::content_at_version(
        prepared.repo(),
        crate::git_engine::NOTES_REF,
        &in_repo,
        &id,
    )
    .map_err(|e| e.message().to_string())?
    .ok_or_else(|| format!("'{rel}' is not part of that recorded state"))?;

    keep_state_of(root_p, target)?;
    atomic_write_bytes(root_p, target, &content).map_err(|e| e.to_string())?;
    log::info!("restore_version: {rel} at {id}");
    Ok(())
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
    let prepared =
        history_repo(root_p)?.ok_or_else(|| format!("'{rel}' is not in this folder's history"))?;
    let repo = prepared.repo();
    let in_repo = prepared.path_in_repo(Path::new(&rel));

    let kept = last_written_at(repo, crate::git_engine::SNAPSHOT_REF, &in_repo)?;
    let recorded = last_written_at(repo, crate::git_engine::NOTES_REF, &in_repo)?;
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

    #[test]
    fn stale_base_allows_a_save_while_the_note_still_holds_its_base() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("a.md");
        std::fs::write(&note, "loaded
").unwrap();
        assert_eq!(stale_base(&note, "loaded
").unwrap(), None);
    }

    #[test]
    fn stale_base_refuses_a_save_over_a_change_made_elsewhere() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("a.md");
        std::fs::write(&note, "changed elsewhere").unwrap();
        assert_eq!(
            stale_base(&note, "loaded").unwrap(),
            Some(WriteOutcome::Conflict { disk: Some("changed elsewhere".into()) })
        );
    }

    #[test]
    fn stale_base_reports_a_note_that_is_gone() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            stale_base(&tmp.path().join("gone.md"), "loaded").unwrap(),
            Some(WriteOutcome::Conflict { disk: None })
        );
    }

    #[test]
    fn stale_base_reports_a_note_whose_folder_is_gone() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            stale_base(&tmp.path().join("box").join("gone.md"), "loaded").unwrap(),
            Some(WriteOutcome::Conflict { disk: None })
        );
    }

    #[test]
    fn write_outcome_crosses_the_boundary_as_a_tagged_object() {
        assert_eq!(
            serde_json::to_value(WriteOutcome::Written).unwrap(),
            serde_json::json!({ "kind": "written" })
        );
        assert_eq!(
            serde_json::to_value(WriteOutcome::Conflict { disk: None }).unwrap(),
            serde_json::json!({ "kind": "conflict", "disk": null })
        );
    }

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
    fn looking_at_history_in_a_plain_folder_leaves_it_a_plain_folder() {
        // Opening a panel is not recording. A folder nothing has been recorded in answers
        // "nothing" and stays exactly as it was found.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let root_s = root.to_string_lossy().to_string();
        let note = seed_note(root, "a.md", "body");

        assert!(deleted_notes(root_s.clone()).unwrap().is_empty());
        assert!(note_versions(root_s.clone(), note.clone()).unwrap().is_empty());
        assert!(note_version_text(root_s.clone(), note, "0".repeat(40)).is_err());
        assert!(restore_deleted(root_s, "a.md".into()).is_err());

        assert!(
            !root.join(".git").exists(),
            "a folder nothing was recorded in is not turned into a repository by reading"
        );
    }

    #[test]
    fn a_deleted_notes_history_can_still_be_read() {
        // Every entry in the deleted list is a name the folder no longer has, so requiring the
        // path to exist would make previewing before restoring impossible.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "sub/a.md", "first");
        delete_node(root_s.clone(), note.clone()).unwrap();
        assert!(!root.join("sub").join("a.md").exists());

        let versions = note_versions(root_s.clone(), note.clone()).unwrap();
        assert_eq!(versions.len(), 1, "the note is gone; its history is not");
        assert_eq!(
            note_version_text(root_s.clone(), note, versions[0].id.clone()).unwrap(),
            "first"
        );

        // The check still refuses what it is there to refuse.
        let outside = TempDir::new().unwrap();
        assert!(note_versions(
            root_s,
            outside.path().join("elsewhere.md").to_string_lossy().to_string()
        )
        .is_err());
    }



    /// Puts a folder into the shape an earlier version of the application left behind.
    fn as_an_earlier_version_left_it(root: &Path, deleted: &[(&str, &str, u64)]) {
        let trash = root.join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(root.join(".textree").join("favorites.json"), "[\"a.md\"]").unwrap();
        let items: Vec<TrashItem> = deleted
            .iter()
            .map(|(name, body, when)| {
                std::fs::write(trash.join(name), body).unwrap();
                TrashItem {
                    trash_name: (*name).to_string(),
                    original_rel: (*name).to_string(),
                    deleted_at: *when,
                    is_dir: false,
                }
            })
            .collect();
        std::fs::write(
            root.join(".textree").join(TRASH_MANIFEST),
            serde_json::to_string(&items).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn moving_state_out_keeps_every_file_and_leaves_the_folder_clean() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();
        as_an_earlier_version_left_it(root, &[("one.md", "first", 1_700_000_000), ("two.md", "second", 1_700_000_500)]);

        let moved = move_state_out_of_vault(root_s.clone()).unwrap();

        assert_eq!(moved.notes, 2, "every set-aside file is accounted for");
        assert!(moved.settings);
        assert!(
            !root.join(".textree").exists(),
            "the notes folder holds only notes afterwards"
        );

        // The count is the point: nothing was thrown away, it was carried.
        let deleted = deleted_notes(root_s.clone()).unwrap();
        assert_eq!(deleted.len(), 2);
        assert_eq!(
            read_sidecar(root_s.clone(), "favorites.json".into()).unwrap(),
            Some("[\"a.md\"]".to_string()),
            "settings are readable from where they live now"
        );

        // And the contents came with them.
        restore_deleted(root_s, "one.md".into()).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("one.md")).unwrap(), "first");
    }

    #[test]
    fn moving_state_out_keeps_when_each_note_left() {
        // The set-aside copies are the only record of that, for anything deleted before the
        // history started keeping it. Dating them all "now" would silently reorder the list.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();
        as_an_earlier_version_left_it(
            root,
            &[("older.md", "a", 1_600_000_000), ("newer.md", "b", 1_700_000_000)],
        );

        move_state_out_of_vault(root_s.clone()).unwrap();

        let listed = deleted_notes(root_s).unwrap();
        assert_eq!(
            listed.iter().map(|d| d.rel.as_str()).collect::<Vec<_>>(),
            vec!["newer.md", "older.md"]
        );
        assert_eq!(listed[1].seconds, 1_600_000_000);
    }

    #[test]
    fn moving_state_out_a_second_time_reports_that_nothing_happened() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();
        as_an_earlier_version_left_it(root, &[("one.md", "first", 1_700_000_000)]);

        assert!(move_state_out_of_vault(root_s.clone()).unwrap().happened());
        let again = move_state_out_of_vault(root_s.clone()).unwrap();
        assert_eq!(again, MoveOut::default(), "there is nothing left to move");
        assert_eq!(deleted_notes(root_s).unwrap().len(), 1, "and nothing was doubled");
    }

    #[test]
    fn a_folder_that_never_held_anything_of_ours_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed_note(root, "a.md", "body");
        let moved = move_state_out_of_vault(root.to_string_lossy().to_string()).unwrap();
        assert_eq!(moved, MoveOut::default());
        assert!(!root.join(".git").exists(), "and no repository is made for it");
    }

    #[test]
    fn going_back_to_an_earlier_state_keeps_the_one_it_replaces() {
        // Going back is the one action that overwrites work rather than adding to it. Work that
        // was never recorded has nowhere else to be, so it has to land somewhere reachable
        // before the older state is written over it.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = record_note(root, "a.md", "first");
        std::fs::write(&note, "worked on since, never recorded").unwrap();

        let first = note_versions(root_s.clone(), note.clone()).unwrap()[0].id.clone();
        restore_version(root_s.clone(), note.clone(), first).unwrap();

        assert_eq!(std::fs::read_to_string(&note).unwrap(), "first");

        // The replaced state is reachable: deleting the note now brings it back, because what
        // was kept is newer than anything recorded.
        delete_node(root_s.clone(), note).unwrap();
        let back = restore_deleted(root_s, "a.md".into()).unwrap();
        assert!(back.as_deleted);
        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "first",
            "and the last state on disk was the one that was put back"
        );
    }

    #[test]
    fn going_back_needs_a_state_that_belongs_to_this_note() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let a = record_note(root, "a.md", "mine");
        let b = record_note(root, "b.md", "someone else's");
        let other = note_versions(root_s.clone(), b).unwrap()[0].id.clone();

        assert!(restore_version(root_s, a.clone(), other).is_err());
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "mine",
            "a refused request leaves the note alone"
        );
    }

    #[test]
    fn a_note_can_only_be_brought_back_from_one_place() {
        // There used to be two: the history, and a copy set aside inside the folder. Taking
        // both offers put two of the same note there — the thing a version history exists to
        // stop. Deleting no longer sets anything aside, so there is one offer.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        git2::Repository::init(root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let note = seed_note(root, "a.md", "body");
        delete_node(root_s.clone(), note).unwrap();

        assert_eq!(deleted_notes(root_s.clone()).unwrap().len(), 1);
        restore_deleted(root_s.clone(), "a.md".into()).unwrap();
        assert!(deleted_notes(root_s).unwrap().is_empty());
        assert!(root.join("a.md").exists());
        assert!(!root.join("a (1).md").exists());
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
    fn settings_live_outside_the_notes_folder_and_stay_confined() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let settings = sidecar_path(root, "favorites.json").unwrap();
        assert!(
            !settings.starts_with(root),
            "a preference is not a note and does not belong in the notes folder"
        );
        assert!(settings.ends_with("favorites.json"));
        assert!(sidecar_path(root, "views/board.json").unwrap().ends_with("board.json"));

        // Two folders that end with the same name still get their own place.
        let same_name = TempDir::new().unwrap();
        std::fs::create_dir(same_name.path().join("notes")).unwrap();
        std::fs::create_dir(root.join("notes")).unwrap();
        assert_ne!(
            sidecar_path(&root.join("notes"), "favorites.json").unwrap(),
            sidecar_path(&same_name.path().join("notes"), "favorites.json").unwrap()
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
