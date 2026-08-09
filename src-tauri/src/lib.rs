mod app;
mod commands;
mod database;
mod domain;
mod downloader;
mod errors;
mod repositories;
mod services;
mod storage;

use tauri::Manager;

pub use domain::download::DownloadStatus;
pub use errors::AppError;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bitig=info".into()),
        )
        .with_target(false)
        .compact()
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let state = tauri::async_runtime::block_on(app::AppState::initialize(&app_handle))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::folders::list_folders,
            commands::folders::create_folder,
            commands::folders::rename_folder,
            commands::folders::delete_folder,
            commands::library::list_media,
            commands::library::get_media,
            commands::library::get_media_playback_path,
            commands::library::get_media_thumbnail_path,
            commands::library::export_media,
            commands::library::search_library,
            commands::library::rename_media,
            commands::library::move_media,
            commands::library::delete_media,
            commands::analysis::analyze_url,
            commands::downloads::start_download,
            commands::downloads::cancel_download,
            commands::downloads::get_download,
            commands::downloads::list_downloads,
            commands::tools::get_tool_status,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run BITIG");
}
