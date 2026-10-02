use std::sync::Arc;

use tauri::AppHandle;

use crate::{
    database,
    downloader::ToolPaths,
    errors::AppResult,
    repositories::{FolderRepository, MediaRepository},
    services::{
        DownloadManager, FolderService, LibraryService, MediaAnalysisService, ToolStatusService,
        ToolUpdateService,
    },
    storage::AppPaths,
};

pub struct AppState {
    pub folders: FolderService,
    pub library: LibraryService,
    pub analysis: MediaAnalysisService,
    pub downloads: DownloadManager,
    pub tool_status: ToolStatusService,
    pub tool_updates: ToolUpdateService,
}

impl AppState {
    pub async fn initialize(app: &AppHandle) -> AppResult<Self> {
        let paths = AppPaths::initialize(app).await?;
        let pool = database::connect(&paths.database).await?;
        database::recover_interrupted_downloads(&pool).await?;
        paths.cleanup_stale_temp().await?;
        let tools = ToolPaths::discover_and_verify(&paths.tools).await?;
        let tool_status = ToolStatusService::new(tools.versions.clone());
        let tool_updates = ToolUpdateService::new(
            paths.tools.clone(),
            paths.temp.clone(),
            tools.clone(),
            tool_status.clone(),
        );

        let folders = Arc::new(FolderRepository::new(pool.clone()));
        let media = Arc::new(MediaRepository::new(pool.clone()));
        let analysis = MediaAnalysisService::new(pool.clone(), tools.yt_dlp.clone());
        let downloads =
            DownloadManager::new(app.clone(), analysis.clone(), pool, paths.clone(), tools);

        Ok(Self {
            folders: FolderService::new(folders, paths.clone()),
            library: LibraryService::new(media, paths),
            analysis,
            downloads,
            tool_status,
            tool_updates,
        })
    }
}
