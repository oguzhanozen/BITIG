use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use tauri::{AppHandle, Manager};

use crate::errors::{AppError, AppResult};

mod installer_preferences;
pub(crate) use installer_preferences::{
    InstallerToolPreference, read_installer_tool_preference, write_installer_tool_preference,
};

pub(crate) fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn reject_linked_directories(path: &Path) -> AppResult<()> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if !metadata.is_dir() || is_link(&metadata) => {
                return Err(AppError::UnsafePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) fn ensure_real_directory(path: &Path) -> AppResult<()> {
    reject_linked_directories(path)?;
    std::fs::create_dir_all(path)?;
    reject_linked_directories(path)
}

pub(crate) fn open_regular_file(path: &Path) -> AppResult<std::fs::File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(AppError::UnsafePath);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // Open the reparse point itself if the file changes after inspection.
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(AppError::UnsafePath);
    }
    Ok(file)
}

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: Arc<PathBuf>,
    pub database: PathBuf,
    pub media: PathBuf,
    pub thumbnails: PathBuf,
    pub temp: PathBuf,
    pub tools: PathBuf,
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
            tools: root.join("tools"),
            logs: root.join("logs"),
            root: Arc::new(root),
        };
        for directory in [
            &paths.media,
            &paths.thumbnails,
            &paths.temp,
            &paths.tools,
            &paths.logs,
        ] {
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

    #[test]
    fn installer_preference_round_trips_without_creating_media_or_database() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            super::read_installer_tool_preference(directory.path()).unwrap(),
            None
        );
        for preference in [
            super::InstallerToolPreference::Enabled,
            super::InstallerToolPreference::Disabled,
        ] {
            super::write_installer_tool_preference(directory.path(), preference).unwrap();
            assert_eq!(
                super::read_installer_tool_preference(directory.path()).unwrap(),
                Some(preference)
            );
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    fn paths() -> AppPaths {
        let root = PathBuf::from("managed");
        AppPaths {
            database: root.join("library.db"),
            media: root.join("media"),
            thumbnails: root.join("thumbnails"),
            temp: root.join("temp"),
            tools: root.join("tools"),
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
        assert!(paths().resolve_managed("media/../../secret").is_err());
        assert!(paths().resolve_managed("..\\secret").is_err());
        assert!(paths().resolve_managed("C:\\secret").is_err());
        assert!(
            paths()
                .resolve_managed("\\\\server\\share\\secret")
                .is_err()
        );
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

    #[tokio::test]
    async fn stale_cleanup_removes_only_owned_job_entries() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let temp = root.join("temp");
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let job_id = "018f0c30-7b14-7cc1-8db7-5d2c557f74cf";
        tokio::fs::create_dir_all(temp.join(job_id)).await.unwrap();
        tokio::fs::write(temp.join(job_id).join("partial.bin"), b"partial")
            .await
            .unwrap();
        tokio::fs::write(temp.join("notes"), b"keep").await.unwrap();
        let app_paths = AppPaths {
            database: root.join("library.db"),
            media: root.join("media"),
            thumbnails: root.join("thumbnails"),
            temp: temp.clone(),
            tools: root.join("tools"),
            logs: root.join("logs"),
            root: Arc::new(root),
        };

        app_paths.cleanup_stale_temp().await.unwrap();

        assert!(!temp.join(job_id).exists());
        assert!(temp.join("notes").exists());
    }
}
