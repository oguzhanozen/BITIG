use tauri::State;

use crate::{app::AppState, domain::ToolStatusReport, errors::AppResult};

#[tauri::command]
pub async fn get_tool_status(
    check_updates: bool,
    state: State<'_, AppState>,
) -> AppResult<ToolStatusReport> {
    state.tool_status.report(check_updates).await
}
