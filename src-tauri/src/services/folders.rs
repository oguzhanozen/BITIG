use std::sync::Arc;

use uuid::Uuid;

use crate::{
    domain::{CreateFolderInput, Folder, RenameInput},
    errors::{AppError, AppResult},
    repositories::FolderRepository,
};

#[derive(Clone)]
pub struct FolderService {
    repository: Arc<FolderRepository>,
}

impl FolderService {
    pub fn new(repository: Arc<FolderRepository>) -> Self {
        Self { repository }
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

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        if !self.repository.exists(id).await? {
            return Err(AppError::NotFound);
        }
        if !self.repository.is_empty(id).await? {
            return Err(AppError::NotEmpty);
        }
        self.repository.delete(id).await
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
        database, domain::CreateFolderInput, errors::AppError, repositories::FolderRepository,
    };

    #[tokio::test]
    async fn migrations_support_nested_folders_and_safe_delete() {
        let directory = tempdir().unwrap();
        let pool = database::connect(&directory.path().join("library.db"))
            .await
            .unwrap();
        let service = FolderService::new(Arc::new(FolderRepository::new(pool)));

        let root = service
            .create(CreateFolderInput {
                name: "Football".into(),
                parent_id: None,
            })
            .await
            .unwrap();
        service
            .create(CreateFolderInput {
                name: "Highlights".into(),
                parent_id: Some(root.id.clone()),
            })
            .await
            .unwrap();

        assert!(matches!(
            service.delete(&root.id).await,
            Err(AppError::NotEmpty)
        ));
        assert_eq!(service.list().await.unwrap().len(), 2);
    }
}
