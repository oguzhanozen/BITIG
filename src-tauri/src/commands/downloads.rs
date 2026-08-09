use tauri::State;

use crate::{
    app::AppState,
    domain::{DownloadJob, StartDownloadInput},
    errors::AppResult,
};

#[tauri::command]
pub async fn start_download(
    input: StartDownloadInput,
    state: State<'_, AppState>,
) -> AppResult<DownloadJob> {
    state.downloads.start(input).await
}

#[tauri::command]
pub async fn cancel_download(id: String, state: State<'_, AppState>) -> AppResult<DownloadJob> {
    state.downloads.cancel(&id).await
}

#[tauri::command]
pub async fn get_download(id: String, state: State<'_, AppState>) -> AppResult<DownloadJob> {
    state.downloads.get(&id).await
}

#[tauri::command]
pub async fn list_downloads(state: State<'_, AppState>) -> AppResult<Vec<DownloadJob>> {
    state.downloads.list().await
}
