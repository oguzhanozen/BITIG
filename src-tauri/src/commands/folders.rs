use tauri::State;

use crate::{
    app::AppState,
    domain::{CreateFolderInput, Folder, FolderDeleteMode, RenameInput, ReorderFolderInput},
    errors::AppResult,
};

#[tauri::command]
pub async fn list_folders(state: State<'_, AppState>) -> AppResult<Vec<Folder>> {
    state.folders.list().await
}

#[tauri::command]
pub async fn create_folder(
    input: CreateFolderInput,
    state: State<'_, AppState>,
) -> AppResult<Folder> {
    state.folders.create(input).await
}

#[tauri::command]
pub async fn rename_folder(input: RenameInput, state: State<'_, AppState>) -> AppResult<Folder> {
    state.folders.rename(input).await
}

#[tauri::command]
pub async fn reorder_folder(
    input: ReorderFolderInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<Folder>> {
    state.folders.reorder(input).await
}

#[tauri::command]
pub async fn delete_folder(
    id: String,
    mode: FolderDeleteMode,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.folders.delete(&id, mode).await
}
