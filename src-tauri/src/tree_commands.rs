//! IPC entry points for the commands that change the tree or read the folder's history — run off
//! the main thread.
//!
//! A command that is not `async` runs on the main thread, and there a folder that stops answering
//! (a stalled sync or network drive) freezes the window, as does a render that takes seconds. Each
//! entry here moves its work to a blocking thread and, when it changes the tree, first takes a
//! turn over the places it changes ([`NoteLocks::subtree`]) so no save lands inside them midway.
//!
//! The work itself lives in [`crate::commands`] under the same names, where tests call it
//! directly. Only places a change reads from need a turn: a save never creates a file (one whose
//! note is not there is refused as gone), so a place a change writes to cannot be raced by one.

use crate::commands::{self, off_main, DeletedNote, MoveOut, NoteVersion, RestoredNote};
use crate::search::IndexHandle;
use crate::note_locks::NoteLocks;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Opening a folder reads all of it; one that does not answer must not hold the window, or keep
/// the person from opening another instead (the later opening wins — see
/// [`commands::open_vault`]).
#[tauri::command]
pub async fn open_vault(root: String, app: AppHandle) -> Result<Vec<crate::vault::TreeNode>, String> {
    let ticket = commands::begin_open();
    off_main(move || commands::open_vault(root, app, ticket)).await
}

/// Runs `work` off the main thread while holding a turn over `places`.
async fn changing<T: Send + 'static>(
    locks: &State<'_, Arc<NoteLocks>>,
    places: Vec<String>,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let locks = locks.inner().clone();
    off_main(move || {
        let places: Vec<&Path> = places.iter().map(Path::new).collect();
        let _turn = locks.subtree(&places);
        work()
    })
    .await
}

#[tauri::command]
pub async fn rename_node(
    root: String,
    path: String,
    name: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<String, String> {
    changing(&locks, vec![path.clone()], move || commands::rename_node(root, path, name)).await
}

#[tauri::command]
pub async fn rename_note_unique(
    root: String,
    path: String,
    name: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<String, String> {
    changing(&locks, vec![path.clone()], move || commands::rename_note_unique(root, path, name))
        .await
}

#[tauri::command]
pub async fn move_node(
    root: String,
    path: String,
    dest: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<String, String> {
    changing(&locks, vec![path.clone()], move || commands::move_node(root, path, dest)).await
}

#[tauri::command]
pub async fn delete_node(
    root: String,
    path: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<(), String> {
    changing(&locks, vec![path.clone()], move || commands::delete_node(root, path)).await
}

/// Promoting replaces the note with a folder of the same name holding it.
#[tauri::command]
pub async fn promote_node(
    root: String,
    path: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<String, String> {
    changing(&locks, vec![path.clone()], move || commands::promote_node(root, path)).await
}

/// Adopting moves `path` under `leaf`, promoting `leaf` first — both are read from.
#[tauri::command]
pub async fn adopt_node(
    root: String,
    path: String,
    leaf: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<String, String> {
    let places = vec![path.clone(), leaf.clone()];
    changing(&locks, places, move || commands::adopt_node(root, path, leaf)).await
}

/// Bringing back an earlier version writes over the note, so it takes the note's turn like a save.
#[tauri::command]
pub async fn restore_version(
    root: String,
    path: String,
    id: String,
    locks: State<'_, Arc<NoteLocks>>,
) -> Result<(), String> {
    changing(&locks, vec![path.clone()], move || commands::restore_version(root, path, id)).await
}

// The rest create what was not there, or only read — nothing a save could land inside of.

#[tauri::command]
pub async fn restore_deleted(root: String, rel: String) -> Result<RestoredNote, String> {
    off_main(move || commands::restore_deleted(root, rel)).await
}

#[tauri::command]
pub async fn create_note(root: String, parent: String, name: String) -> Result<String, String> {
    off_main(move || commands::create_note(root, parent, name)).await
}

#[tauri::command]
pub async fn create_untitled_note(root: String, parent: String) -> Result<String, String> {
    off_main(move || commands::create_untitled_note(root, parent)).await
}

#[tauri::command]
pub async fn create_note_with_content(
    root: String,
    parent: String,
    name: String,
    content: String,
) -> Result<String, String> {
    off_main(move || commands::create_note_with_content(root, parent, name, content)).await
}

#[tauri::command]
pub async fn create_folder(root: String, parent: String, name: String) -> Result<String, String> {
    off_main(move || commands::create_folder(root, parent, name)).await
}

#[tauri::command]
pub async fn save_attachment(
    root: String,
    note: String,
    data: String,
    ext: String,
) -> Result<String, String> {
    off_main(move || commands::save_attachment(root, note, data, ext)).await
}

/// Recording reads the notes and changes none of them: a save running meanwhile is recorded as it
/// was either just before or just after — each a state the note really held.
#[tauri::command]
pub async fn commit_notes(
    root: String,
    paths: Vec<String>,
    message: String,
) -> Result<Option<String>, String> {
    off_main(move || commands::commit_notes(root, paths, message)).await
}

#[tauri::command]
pub async fn move_state_out_of_vault(root: String) -> Result<MoveOut, String> {
    off_main(move || commands::move_state_out_of_vault(root)).await
}

#[tauri::command]
pub async fn note_versions(root: String, path: String) -> Result<Vec<NoteVersion>, String> {
    off_main(move || commands::note_versions(root, path)).await
}

#[tauri::command]
pub async fn note_version_text(root: String, path: String, id: String) -> Result<String, String> {
    off_main(move || commands::note_version_text(root, path, id)).await
}

#[tauri::command]
pub async fn deleted_notes(root: String) -> Result<Vec<DeletedNote>, String> {
    off_main(move || commands::deleted_notes(root)).await
}

#[tauri::command]
pub async fn rebuild_index(root: String, index: State<'_, Arc<IndexHandle>>) -> Result<(), String> {
    let index = index.inner().clone();
    off_main(move || commands::rebuild_index(root, &index)).await
}

/// Rendering can take seconds (much longer on a cold start); the window stays responsive meanwhile.
#[tauri::command]
pub async fn publish_site(
    app: AppHandle,
    vault_path: String,
    out_dir: String,
    options: crate::publish::PublishOptions,
) -> Result<crate::publish::PublishResult, String> {
    off_main(move || commands::publish_site(app, vault_path, out_dir, options)).await
}

#[tauri::command]
pub async fn publish_to_cloud(
    app: AppHandle,
    vault_path: String,
    options: crate::publish::PublishOptions,
) -> Result<crate::cloud_publish::PublishToCloudResult, String> {
    off_main(move || commands::publish_to_cloud(app, vault_path, options)).await
}
