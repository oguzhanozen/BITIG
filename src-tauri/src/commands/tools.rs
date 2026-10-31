use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    app::AppState,
    domain::{ToolStatusReport, ToolUpdateResult},
    errors::AppResult,
    storage::{InstallerToolPreference, read_installer_tool_preference},
};

#[tauri::command]
pub async fn get_installer_tool_preference(
    app: AppHandle,
) -> AppResult<Option<InstallerToolPreference>> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| crate::errors::AppError::Storage(error.to_string()))?;
    tokio::task::spawn_blocking(move || read_installer_tool_preference(&root))
        .await
        .map_err(|error| crate::errors::AppError::Storage(error.to_string()))?
}

#[tauri::command]
pub async fn get_tool_status(
    check_updates: bool,
    state: State<'_, AppState>,
) -> AppResult<ToolStatusReport> {
    state.tool_status.report(check_updates).await
}

#[tauri::command]
pub async fn install_tool_updates(
    operation_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ToolUpdateResult> {
    state
        .tool_updates
        .install_available(operation_id, move |progress| {
            if let Err(error) = app.emit("tool-update://progress", progress) {
                tracing::warn!(%error, "tool update progress could not be emitted");
            }
        })
        .await
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}
