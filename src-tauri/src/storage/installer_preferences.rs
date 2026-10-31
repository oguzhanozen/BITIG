use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::{ensure_real_directory, is_link, open_regular_file, reject_linked_directories};
use crate::errors::{AppError, AppResult};

const FILE_NAME: &str = "installer-preferences.json";
const MAX_BYTES: u64 = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallerToolPreference {
    Enabled,
    Disabled,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallerPreferences {
    schema_version: u32,
    auto_update_tools: InstallerToolPreference,
}

pub(crate) fn read_installer_tool_preference(
    root: &Path,
) -> AppResult<Option<InstallerToolPreference>> {
    reject_linked_directories(root)?;
    let path = root.join(FILE_NAME);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_BYTES {
        return Err(AppError::Validation(
            "invalid installer preference file".into(),
        ));
    }
    let mut bytes = Vec::new();
    open_regular_file(&path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(AppError::Validation(
            "installer preference file is too large".into(),
        ));
    }
    let preferences: InstallerPreferences = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Validation("invalid installer preference file".into()))?;
    if preferences.schema_version != 1 {
        return Err(AppError::Validation(
            "unsupported installer preference schema".into(),
        ));
    }
    Ok(Some(preferences.auto_update_tools))
}

pub(crate) fn write_installer_tool_preference(
    root: &Path,
    preference: InstallerToolPreference,
) -> AppResult<()> {
    ensure_real_directory(root)?;
    let destination = root.join(FILE_NAME);
    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) if !metadata.is_file() || is_link(&metadata) => {
            return Err(AppError::UnsafePath);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let bytes = serde_json::to_vec(&InstallerPreferences {
        schema_version: 1,
        auto_update_tools: preference,
    })
    .map_err(|_| AppError::Validation("installer preference could not be serialized".into()))?;
    let temporary = root.join(format!(
        ".installer-preferences-{}.tmp",
        uuid::Uuid::now_v7()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_preferences_are_distinct_from_invalid_or_oversized_records() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            read_installer_tool_preference(directory.path()).unwrap(),
            None
        );
        for body in [
            "{",
            "null",
            "{}",
            r#"{"schemaVersion":2,"autoUpdateTools":"enabled"}"#,
            r#"{"schemaVersion":1,"autoUpdateTools":"undecided"}"#,
            r#"{"schemaVersion":1,"autoUpdateTools":true}"#,
            r#"{"schemaVersion":1,"autoUpdateTools":"enabled","unexpected":true}"#,
            &" ".repeat(MAX_BYTES as usize + 1),
        ] {
            std::fs::write(directory.path().join(FILE_NAME), body).unwrap();
            assert!(
                read_installer_tool_preference(directory.path()).is_err(),
                "{body}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_atomic_replace_preserves_the_previous_preference_and_removes_staging() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        write_installer_tool_preference(directory.path(), InstallerToolPreference::Enabled)
            .unwrap();
        let old = std::fs::read(directory.path().join(FILE_NAME)).unwrap();
        let guard = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(directory.path().join(FILE_NAME))
            .unwrap();
        assert!(read_installer_tool_preference(directory.path()).is_err());
        assert!(
            write_installer_tool_preference(directory.path(), InstallerToolPreference::Disabled)
                .is_err()
        );
        drop(guard);
        assert_eq!(
            std::fs::read(directory.path().join(FILE_NAME)).unwrap(),
            old
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn linked_preference_directories_are_never_read_or_written() {
        use std::os::windows::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("real");
        write_installer_tool_preference(&target, InstallerToolPreference::Enabled).unwrap();
        let link = directory.path().join("linked");
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(read_installer_tool_preference(&link).is_err());
        assert!(write_installer_tool_preference(&link, InstallerToolPreference::Disabled).is_err());
        assert_eq!(
            read_installer_tool_preference(&target).unwrap(),
            Some(InstallerToolPreference::Enabled)
        );
        std::fs::remove_dir(link).unwrap();
    }
}
