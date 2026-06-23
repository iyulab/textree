pub(crate) mod commands;
pub(crate) mod host;
pub(crate) mod fs_ops;
pub(crate) mod pathsafe;
pub(crate) mod publish;
pub(crate) mod search;
pub(crate) mod self_write;
pub(crate) mod vault;
pub(crate) mod watcher;

use host::HostHandle;
use search::IndexHandle;
use self_write::SelfWrites;
use std::sync::Arc;
use watcher::WatcherHandle;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("textree".into()),
                    },
                ))
                .level(log::LevelFilter::Info)
                // Rotation: keep at most one rotated file (~5 MB ceiling).
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Log startup here (after plugin init) so the logger is already wired.
            log::info!("textree starting (v{})", env!("CARGO_PKG_VERSION"));
            #[cfg(windows)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            // Spawn the local AI host when TEXTREE_HOST_EXE is set (dev/production opt-in).
            // If the env var is absent the handle stays Unavailable — graceful degradation.
            if let Ok(exe) = std::env::var("TEXTREE_HOST_EXE") {
                use tauri::Manager;
                let handle = app.state::<std::sync::Arc<HostHandle>>().inner().clone();
                if let Ok(log_dir) = app.path().app_log_dir() {
                    host::spawn_host(handle, exe, log_dir);
                }
            }
            Ok(())
        })
        // Managed via Arc so the watcher thread and the write_note command share one registry.
        .manage(Arc::new(SelfWrites::default()))
        .manage(WatcherHandle::default())
        .manage(Arc::new(IndexHandle::default()))
        .manage(Arc::new(HostHandle::default()))
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::list_tree,
            commands::read_note,
            commands::write_note,
            commands::create_note,
            commands::create_folder,
            commands::promote_node,
            commands::delete_node,
            commands::restore_node,
            commands::rename_node,
            commands::move_node,
            commands::adopt_node,
            commands::save_attachment,
            commands::read_sidecar,
            commands::write_sidecar,
            commands::search_content,
            commands::rebuild_index,
            commands::publish_site,
            commands::list_trash,
            commands::purge_trash,
            commands::ensure_default_vault,
            commands::open_log_dir,
            host::host_status,
            host::semantic_search,
            host::prepare_ai_model,
            host::ask,
            host::cancel_ask,
            host::prepare_generation
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                log::info!("textree shutting down");
                use tauri::Manager;
                let handle = app_handle.state::<std::sync::Arc<HostHandle>>();
                host::shutdown_host(&handle);
            }
        });
}
