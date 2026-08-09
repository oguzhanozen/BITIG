use tauri::State;

use crate::{
    app::AppState,
    domain::{Media, MoveMediaInput, RenameInput},
    errors::AppResult,
};

#[tauri::command]
pub async fn list_media(
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<Media>> {
    state.library.list(folder_id.as_deref()).await
}

#[tauri::command]
pub async fn get_media(id: String, state: State<'_, AppState>) -> AppResult<Media> {
    state.library.get(&id).await
}

#[tauri::command]
pub async fn get_media_playback_path(id: String, state: State<'_, AppState>) -> AppResult<String> {
    state.library.playback_path(&id).await
}

#[tauri::command]
pub async fn get_media_thumbnail_path(
    id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    state.library.thumbnail_path(&id).await
}

#[tauri::command]
pub async fn export_media(
    id: String,
    destination: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.library.export(&id, &destination).await
}

#[tauri::command]
pub async fn search_library(query: String, state: State<'_, AppState>) -> AppResult<Vec<Media>> {
    state.library.search(&query).await
}

#[tauri::command]
pub async fn rename_media(input: RenameInput, state: State<'_, AppState>) -> AppResult<Media> {
    state.library.rename(input).await
}

#[tauri::command]
pub async fn move_media(input: MoveMediaInput, state: State<'_, AppState>) -> AppResult<Media> {
    state.library.move_to(input).await
}

#[tauri::command]
pub async fn delete_media(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.library.delete(&id).await
}
