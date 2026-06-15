pub(crate) mod commands;
pub(crate) mod fs_ops;
pub(crate) mod pathsafe;
pub(crate) mod search;
pub(crate) mod self_write;
pub(crate) mod vault;
pub(crate) mod watcher;

use search::IndexHandle;
use self_write::SelfWrites;
use std::sync::Arc;
use watcher::WatcherHandle;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 워처 스레드와 write_note 커맨드가 같은 레지스트리를 공유하도록 Arc로 관리.
        .manage(Arc::new(SelfWrites::default()))
        .manage(WatcherHandle::default())
        .manage(Arc::new(IndexHandle::default()))
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::list_tree,
            commands::read_note,
            commands::write_note,
            commands::create_note,
            commands::create_folder,
            commands::promote_node,
            commands::delete_node,
            commands::rename_node,
            commands::move_node,
            commands::adopt_node,
            commands::save_attachment,
            commands::read_sidecar,
            commands::write_sidecar,
            commands::search_content,
            commands::rebuild_index
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
