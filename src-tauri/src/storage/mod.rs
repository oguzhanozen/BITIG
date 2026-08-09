use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use tauri::{AppHandle, Manager};

use crate::errors::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: Arc<PathBuf>,
    pub database: PathBuf,
    pub media: PathBuf,
    pub thumbnails: PathBuf,
    pub temp: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    pub async fn initialize(app: &AppHandle) -> AppResult<Self> {
        let root = app
            .path()
            .app_local_data_dir()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        let paths = Self {
            database: root.join("library.db"),
            media: root.join("media"),
            thumbnails: root.join("thumbnails"),
            temp: root.join("temp"),
            logs: root.join("logs"),
            root: Arc::new(root),
        };
        for directory in [&paths.media, &paths.thumbnails, &paths.temp, &paths.logs] {
            tokio::fs::create_dir_all(directory).await?;
        }
        Ok(paths)
    }

    pub fn resolve_managed(&self, relative: &str) -> AppResult<PathBuf> {
        let path = Path::new(relative);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(AppError::UnsafePath);
        }
        Ok(self.root.join(path))
    }

    pub async fn cleanup_stale_temp(&self) -> AppResult<()> {
        let mut entries = tokio::fs::read_dir(&self.temp).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !is_temp_job_name(name) {
                tracing::warn!(name, "unrecognized temp entry was left untouched");
                continue;
            }
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                tokio::fs::remove_dir_all(entry.path()).await?;
            } else {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }
        Ok(())
    }
}

fn is_temp_job_name(name: &str) -> bool {
    uuid::Uuid::parse_str(name).is_ok()
        || name
            .strip_prefix("delete-")
            .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
}

#[cfg(test)]
mod tests {
    use super::{AppPaths, is_temp_job_name};
    use std::{path::PathBuf, sync::Arc};

    fn paths() -> AppPaths {
        let root = PathBuf::from("managed");
        AppPaths {
            database: root.join("library.db"),
            media: root.join("media"),
            thumbnails: root.join("thumbnails"),
            temp: root.join("temp"),
            logs: root.join("logs"),
            root: Arc::new(root),
        }
    }

    #[test]
    fn accepts_relative_managed_paths() {
        assert_eq!(
            paths().resolve_managed("media/id.mp4").unwrap(),
            PathBuf::from("managed/media/id.mp4")
        );
    }

    #[test]
    fn rejects_parent_traversal_and_absolute_paths() {
        assert!(paths().resolve_managed("../secret").is_err());
        assert!(paths().resolve_managed("C:\\secret").is_err());
    }

    #[test]
    fn only_recognizes_application_temp_names() {
        assert!(is_temp_job_name("018f0c30-7b14-7cc1-8db7-5d2c557f74cf"));
        assert!(is_temp_job_name(
            "delete-018f0c30-7b14-7cc1-8db7-5d2c557f74cf"
        ));
        assert!(!is_temp_job_name("notes"));
        assert!(!is_temp_job_name("../media"));
    }
}
