use tauri::{AppHandle, State};

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
pub async fn install_tool_updates(state: State<'_, AppState>) -> AppResult<ToolUpdateResult> {
    state.tool_updates.install_available().await
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}
