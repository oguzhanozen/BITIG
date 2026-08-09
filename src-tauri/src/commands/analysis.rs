use tauri::State;

use crate::{app::AppState, domain::MediaAnalysis, errors::AppResult};

#[tauri::command]
pub async fn analyze_url(url: String, state: State<'_, AppState>) -> AppResult<MediaAnalysis> {
    state.analysis.analyze(&url).await
}
