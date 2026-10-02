use tauri::{AppHandle, Emitter, State};

use crate::{
    app::AppState,
    domain::{ToolStatusReport, ToolUpdateResult},
    errors::AppResult,
};

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
