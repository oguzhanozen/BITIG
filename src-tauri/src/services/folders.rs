use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use uuid::Uuid;

use crate::{
    domain::{CreateFolderInput, Folder, FolderDeleteMode, RenameInput, ReorderFolderInput},
    errors::{AppError, AppResult},
    repositories::FolderRepository,
    storage::AppPaths,
};

#[derive(Clone)]
pub struct FolderService {
    repository: Arc<FolderRepository>,
    paths: AppPaths,
}

impl FolderService {
    pub fn new(repository: Arc<FolderRepository>, paths: AppPaths) -> Self {
        Self { repository, paths }
    }

    pub async fn list(&self) -> AppResult<Vec<Folder>> {
        self.repository.list().await
    }

    pub async fn create(&self, input: CreateFolderInput) -> AppResult<Folder> {
        let name = validate_name(&input.name, "folder name", 120)?;
        if let Some(parent_id) = input.parent_id.as_deref()
            && !self.repository.exists(parent_id).await?
        {
            return Err(AppError::Validation("parent folder does not exist".into()));
        }
        let id = Uuid::now_v7().to_string();
        self.repository
            .create(&id, input.parent_id.as_deref(), name)
            .await
    }

    pub async fn rename(&self, input: RenameInput) -> AppResult<Folder> {
        let name = validate_name(&input.name, "folder name", 120)?;
        self.repository.rename(&input.id, name).await
    }

    pub async fn reorder(&self, input: ReorderFolderInput) -> AppResult<Vec<Folder>> {
        self.repository
            .reorder(&input.id, &input.target_id, input.placement)
            .await?;
        self.repository.list().await
    }

    pub async fn delete(&self, id: &str, mode: FolderDeleteMode) -> AppResult<()> {
        match mode {
            FolderDeleteMode::MoveContents => self.repository.delete(id).await,
            FolderDeleteMode::DeleteContents => self.delete_with_contents(id).await,
        }
    }

    async fn delete_with_contents(&self, id: &str) -> AppResult<()> {
        let plan = self.repository.begin_delete_contents(id).await?;
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        for (media, thumbnail) in &plan.media_paths {
            let path = managed_file_path(&self.paths, media, "media")?;
            if seen.insert(path.clone()) {
                paths.push((path, &self.paths.media));
            }
            if let Some(thumbnail) = thumbnail {
                let path = managed_file_path(&self.paths, thumbnail, "thumbnails")?;
                if seen.insert(path.clone()) {
                    paths.push((path, &self.paths.thumbnails));
                }
            }
        }

        // An interrupted transaction must leave quarantined files recoverable. The
        // startup temp cleanup intentionally does not recognize this prefix.
        let quarantine = self
            .paths
            .temp
            .join(format!("folder-delete-{}", Uuid::now_v7()));
        tokio::fs::create_dir_all(&quarantine).await?;
        let mut staged = Vec::new();
        for (path, expected_root) in paths {
            let metadata = match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    rollback_staged(&staged, &quarantine).await;
                    return Err(error.into());
                }
            };
            if !metadata.is_file() {
                rollback_staged(&staged, &quarantine).await;
                return Err(AppError::UnsafePath);
            }
            let parent = path.parent().expect("managed file has a parent");
            let actual_parent = tokio::fs::canonicalize(parent).await;
            let expected_parent = tokio::fs::canonicalize(expected_root).await;
            match (actual_parent, expected_parent) {
                (Ok(actual), Ok(expected)) if actual == expected => {}
                (Ok(_), Ok(_)) => {
                    rollback_staged(&staged, &quarantine).await;
                    return Err(AppError::UnsafePath);
                }
                (Err(error), _) | (_, Err(error)) => {
                    rollback_staged(&staged, &quarantine).await;
                    return Err(error.into());
                }
            }

            let relative = path
                .strip_prefix(self.paths.root.as_ref())
                .expect("managed file remains inside app data");
            let destination = quarantine.join(relative);
            if let Some(parent) = destination.parent()
                && let Err(error) = tokio::fs::create_dir_all(parent).await
            {
                rollback_staged(&staged, &quarantine).await;
                return Err(error.into());
            }
            if let Err(error) = tokio::fs::rename(&path, &destination).await {
                rollback_staged(&staged, &quarantine).await;
                return Err(error.into());
            }
            staged.push((path, destination));
        }

        if let Err(error) = self.repository.commit_delete_contents(plan).await {
            rollback_staged(&staged, &quarantine).await;
            return Err(error);
        }
        let cleanup_path = self.paths.temp.join(format!("delete-{}", Uuid::now_v7()));
        match tokio::fs::rename(&quarantine, &cleanup_path).await {
            Ok(()) => {
                if let Err(error) = tokio::fs::remove_dir_all(&cleanup_path).await {
                    tracing::warn!(%error, path = %cleanup_path.display(), "deleted folder quarantine cleanup failed");
                }
            }
            Err(error) => {
                tracing::warn!(%error, path = %quarantine.display(), "deleted folder quarantine remains for recovery");
            }
        }
        Ok(())
    }
}

fn managed_file_path(paths: &AppPaths, relative: &str, directory: &str) -> AppResult<PathBuf> {
    if Path::new(relative).parent() != Some(Path::new(directory)) {
        return Err(AppError::UnsafePath);
    }
    paths.resolve_managed(relative)
}

async fn rollback_staged(staged: &[(PathBuf, PathBuf)], quarantine: &Path) {
    let mut restored = true;
    for (original, staged_path) in staged.iter().rev() {
        if let Err(error) = tokio::fs::rename(staged_path, original).await {
            restored = false;
            tracing::error!(%error, path = %original.display(), "folder deletion rollback could not restore media");
        }
    }
    if restored && let Err(error) = tokio::fs::remove_dir_all(quarantine).await {
        tracing::warn!(%error, path = %quarantine.display(), "folder deletion rollback cleanup failed");
    }
}

pub(crate) fn validate_name<'a>(value: &'a str, label: &str, max: usize) -> AppResult<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(AppError::Validation(format!(
            "{label} must be between 1 and {max} characters"
        )));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::FolderService;
    use crate::{
        database,
        domain::{CreateFolderInput, FolderDeleteMode},
        errors::AppError,
        repositories::FolderRepository,
        storage::AppPaths,
    };

    #[tokio::test]
    async fn migrations_support_nested_folders_and_safe_delete() {
        let directory = tempdir().unwrap();
        let pool = database::connect(&directory.path().join("library.db"))
            .await
            .unwrap();
        let storage_root = directory.path().to_path_buf();
        let service = FolderService::new(
            Arc::new(FolderRepository::new(pool.clone())),
            AppPaths {
                database: storage_root.join("library.db"),
                media: storage_root.join("media"),
                thumbnails: storage_root.join("thumbnails"),
                temp: storage_root.join("temp"),
                tools: storage_root.join("tools"),
                logs: storage_root.join("logs"),
                root: Arc::new(storage_root.clone()),
            },
        );

        let root = service
            .create(CreateFolderInput {
                name: "Football".into(),
                parent_id: None,
            })
            .await
            .unwrap();
        let child = service
            .create(CreateFolderInput {
                name: "Highlights".into(),
                parent_id: Some(root.id.clone()),
            })
            .await
            .unwrap();
        service
            .create(CreateFolderInput {
                name: "Highlights".into(),
                parent_id: None,
            })
            .await
            .unwrap();

        assert!(
            service
                .list()
                .await
                .unwrap()
                .iter()
                .find(|folder| folder.id == root.id)
                .unwrap()
                .has_contents
        );
        service
            .delete(&root.id, FolderDeleteMode::MoveContents)
            .await
            .unwrap();
        let folders = service.list().await.unwrap();
        assert_eq!(folders.len(), 2);
        let moved_child = folders.iter().find(|folder| folder.id == child.id).unwrap();
        assert_eq!(moved_child.parent_id, None);
        assert_eq!(moved_child.name, "Highlights (2)");

        let grandchild = service
            .create(CreateFolderInput {
                name: "Clips".into(),
                parent_id: Some(child.id.clone()),
            })
            .await
            .unwrap();
        tokio::fs::create_dir_all(storage_root.join("media"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(storage_root.join("thumbnails"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(storage_root.join("temp"))
            .await
            .unwrap();
        let media_path = storage_root.join("media/clip.mp4");
        let thumbnail_path = storage_root.join("thumbnails/clip.webp");
        tokio::fs::write(&media_path, b"media").await.unwrap();
        tokio::fs::write(&thumbnail_path, b"thumbnail")
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO media (id, folder_id, title, media_type, source_url, source_platform,
                                source_id, file_path, thumbnail_path, container, file_size)
             VALUES ('clip', ?, 'Clip', 'video', 'https://example.com/clip', 'example', 'clip',
                     'media/clip.mp4', 'thumbnails/clip.webp', 'mp4', 5)",
        )
        .bind(&grandchild.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO downloads (id, folder_id, title, source_url, status)
             VALUES ('pending', ?, 'Pending', 'https://example.com/pending', 'queued')",
        )
        .bind(&grandchild.id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(matches!(
            service
                .delete(&child.id, FolderDeleteMode::DeleteContents)
                .await,
            Err(AppError::Conflict(_))
        ));
        assert!(media_path.exists());
        sqlx::query("UPDATE downloads SET status = 'cancelled' WHERE id = 'pending'")
            .execute(&pool)
            .await
            .unwrap();

        service
            .delete(&child.id, FolderDeleteMode::DeleteContents)
            .await
            .unwrap();
        assert!(!media_path.exists());
        assert!(!thumbnail_path.exists());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(service.list().await.unwrap().len(), 1);
    }
}
