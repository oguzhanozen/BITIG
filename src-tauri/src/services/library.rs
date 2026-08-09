use std::path::{Component, Path};
use std::sync::Arc;

use crate::{
    domain::{Media, MoveMediaInput, RenameInput},
    errors::{AppError, AppResult},
    repositories::MediaRepository,
    storage::AppPaths,
};

use super::folders::validate_name;

#[derive(Clone)]
pub struct LibraryService {
    repository: Arc<MediaRepository>,
    paths: AppPaths,
}

impl LibraryService {
    pub fn new(repository: Arc<MediaRepository>, paths: AppPaths) -> Self {
        Self { repository, paths }
    }

    pub async fn list(&self, folder_id: Option<&str>) -> AppResult<Vec<Media>> {
        self.repository.list(folder_id).await
    }

    pub async fn get(&self, id: &str) -> AppResult<Media> {
        self.repository.get(id).await
    }

    pub async fn playback_path(&self, id: &str) -> AppResult<String> {
        let media = self.repository.get(id).await?;
        let path = self.paths.resolve_managed(&media.file_path)?;
        if !path.is_file() {
            return Err(AppError::NotFound);
        }
        Ok(path.to_string_lossy().into_owned())
    }

    pub async fn thumbnail_path(&self, id: &str) -> AppResult<Option<String>> {
        let media = self.repository.get(id).await?;
        let Some(relative_path) = media.thumbnail_path else {
            return Ok(None);
        };
        let path = self.paths.resolve_managed(&relative_path)?;
        if !path.is_file() {
            return Ok(None);
        }
        Ok(Some(path.to_string_lossy().into_owned()))
    }

    pub async fn export(&self, id: &str, destination: &str) -> AppResult<()> {
        let media = self.repository.get(id).await?;
        let source = self.paths.resolve_managed(&media.file_path)?;
        let destination = Path::new(destination);
        if !destination.is_absolute()
            || destination
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(AppError::Validation("export destination is invalid".into()));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| AppError::Validation("export destination has no parent".into()))?;
        let canonical_parent = tokio::fs::canonicalize(parent).await?;
        let canonical_root = tokio::fs::canonicalize(self.paths.root.as_ref()).await?;
        if canonical_parent.starts_with(&canonical_root)
            || (destination.exists()
                && tokio::fs::canonicalize(destination)
                    .await?
                    .starts_with(&canonical_root))
        {
            return Err(AppError::UnsafePath);
        }
        let expected_extension = Path::new(&media.file_path).extension();
        let extension_matches = destination
            .extension()
            .and_then(|value| value.to_str())
            .zip(expected_extension.and_then(|value| value.to_str()))
            .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected));
        if !extension_matches {
            return Err(AppError::Validation(
                "export file extension must match the media container".into(),
            ));
        }
        tokio::fs::copy(source, destination).await?;
        Ok(())
    }

    pub async fn search(&self, query: &str) -> AppResult<Vec<Media>> {
        let query = query.trim();
        if query.is_empty() {
            return self.list(None).await;
        }
        if query.chars().count() > 200 {
            return Err(AppError::Validation("search query is too long".into()));
        }
        self.repository.search(query).await
    }

    pub async fn rename(&self, input: RenameInput) -> AppResult<Media> {
        let title = validate_name(&input.name, "media title", 240)?;
        self.repository.rename(&input.id, title).await
    }

    pub async fn move_to(&self, input: MoveMediaInput) -> AppResult<Media> {
        self.repository
            .move_to(&input.id, input.folder_id.as_deref())
            .await
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        let media = self.repository.get(id).await?;
        let media_path = self.paths.resolve_managed(&media.file_path)?;
        let thumbnail_path = media
            .thumbnail_path
            .as_deref()
            .map(|path| self.paths.resolve_managed(path))
            .transpose()?;

        let quarantine = self
            .paths
            .temp
            .join(format!("delete-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&quarantine).await?;
        let quarantined_media = quarantine.join("media");
        let quarantined_thumbnail = quarantine.join("thumbnail");

        let media_moved = if media_path.exists() {
            tokio::fs::rename(&media_path, &quarantined_media).await?;
            true
        } else {
            false
        };
        let thumbnail_moved =
            if let Some(path) = thumbnail_path.as_ref().filter(|path| path.exists()) {
                if let Err(error) = tokio::fs::rename(path, &quarantined_thumbnail).await {
                    if media_moved {
                        let _ = tokio::fs::rename(&quarantined_media, &media_path).await;
                    }
                    return Err(error.into());
                }
                true
            } else {
                false
            };

        if let Err(error) = self.repository.delete(id).await {
            if media_moved {
                let _ = tokio::fs::rename(&quarantined_media, &media_path).await;
            }
            if thumbnail_moved && let Some(path) = thumbnail_path {
                let _ = tokio::fs::rename(&quarantined_thumbnail, path).await;
            }
            return Err(error);
        }
        if let Err(error) = tokio::fs::remove_dir_all(&quarantine).await {
            tracing::warn!(%error, path = %quarantine.display(), "deleted media quarantine cleanup failed");
        }
        Ok(())
    }
}
