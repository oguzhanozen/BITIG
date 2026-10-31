use std::{
    ffi::OsString,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    domain::{ToolUpdateProgress, ToolUpdateStage},
    errors::{AppError, AppResult},
    services::ToolUpdateService,
    storage::{InstallerToolPreference, write_installer_tool_preference},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallerAction {
    Bootstrap(InstallerToolPreference),
    DeferredBootstrap(InstallerToolPreference),
    CommitPreferences(InstallerToolPreference),
}

fn parse_args(args: &[OsString]) -> AppResult<Option<InstallerAction>> {
    if !args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg.starts_with("--installer-bootstrap")
            || arg.starts_with("--installer-commit-preferences")
            || arg.starts_with("--auto-update")
            || arg == "--defer-preferences"
    }) {
        return Ok(None);
    }
    let preference = match args.get(1).and_then(|argument| argument.to_str()) {
        Some("--auto-update=enabled") => Some(InstallerToolPreference::Enabled),
        Some("--auto-update=disabled") => Some(InstallerToolPreference::Disabled),
        _ => None,
    };
    if let Some(preference) = preference {
        let action = match args {
            [mode, _] if mode == "--installer-bootstrap" => {
                Some(InstallerAction::Bootstrap(preference))
            }
            [mode, _, defer]
                if mode == "--installer-bootstrap" && defer == "--defer-preferences" =>
            {
                Some(InstallerAction::DeferredBootstrap(preference))
            }
            [mode, _] if mode == "--installer-commit-preferences" => {
                Some(InstallerAction::CommitPreferences(preference))
            }
            _ => None,
        };
        if action.is_some() {
            return Ok(action);
        }
    }
    Err(AppError::Validation(
        "use --installer-bootstrap --auto-update=enabled|disabled [--defer-preferences] or --installer-commit-preferences --auto-update=enabled|disabled".into(),
    ))
}

pub(crate) fn run_if_requested(args: Vec<OsString>) -> Option<i32> {
    let action = match parse_args(&args) {
        Ok(None) => return None,
        Ok(Some(preference)) => preference,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            return Some(2);
        }
    };
    let result = (|| {
        let root = bootstrap_data_dir()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(bootstrap_at(root, action, std::io::stdout()))
    })();
    match result {
        Ok(()) => Some(0),
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "Media tool setup failed: {error}");
            Some(1)
        }
    }
}

async fn bootstrap_at(
    root: PathBuf,
    action: InstallerAction,
    writer: impl Write + Send + 'static,
) -> AppResult<()> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(AppError::ToolUpdate(
            "installer bootstrap supports Windows x64 only".into(),
        ));
    }
    let (preference, defer_preferences) = match action {
        InstallerAction::Bootstrap(preference) => (preference, false),
        InstallerAction::DeferredBootstrap(preference) => (preference, true),
        InstallerAction::CommitPreferences(preference) => {
            return write_installer_tool_preference(&root, preference);
        }
    };
    let output = Arc::new(Mutex::new(BootstrapOutput { writer, last: None }));
    let progress_output = output.clone();
    ToolUpdateService::provision_required_tools(
        root.join("tools"),
        root.join("temp"),
        move |progress| {
            if let Ok(mut output) = progress_output.lock() {
                let _ = output.progress(&progress);
            }
        },
    )
    .await?;
    if !defer_preferences {
        write_installer_tool_preference(&root, preference)?;
    }
    // GUI-subsystem launches may lack output handles; readiness depends on tools and preferences.
    if let Ok(mut output) = output.lock() {
        let _ = output.line("Required media tools are ready.");
    }
    Ok(())
}

struct BootstrapOutput<W> {
    writer: W,
    last: Option<&'static str>,
}

impl<W: Write> BootstrapOutput<W> {
    fn line(&mut self, line: &'static str) -> std::io::Result<()> {
        if self.last != Some(line) {
            writeln!(self.writer, "{line}")?;
            self.writer.flush()?;
            self.last = Some(line);
        }
        Ok(())
    }

    fn progress(&mut self, progress: &ToolUpdateProgress) -> std::io::Result<()> {
        self.line(match progress.stage {
            ToolUpdateStage::Checking => "Checking existing BITIG tools...",
            ToolUpdateStage::Downloading if progress.tool_name.as_deref() == Some("yt-dlp") => {
                "Downloading yt-dlp..."
            }
            ToolUpdateStage::Downloading => "Downloading FFmpeg...",
            ToolUpdateStage::Verifying => "Verifying SHA-256...",
            ToolUpdateStage::Installing => "Installing verified media tools...",
        })
    }
}

#[cfg(windows)]
fn bootstrap_data_dir() -> AppResult<PathBuf> {
    use std::{
        ffi::{OsString, c_void},
        os::windows::ffi::OsStringExt,
    };
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            id: &Guid,
            flags: u32,
            token: *mut c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(memory: *mut c_void);
    }
    let local_app_data = Guid {
        data1: 0xf1b32785,
        data2: 0x6fba,
        data3: 0x4fcf,
        data4: [0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91],
    };
    let mut raw = std::ptr::null_mut();
    // Match Tauri's dirs::data_local_dir without constructing an AppHandle or runtime.
    let local = unsafe {
        let result = SHGetKnownFolderPath(&local_app_data, 0, std::ptr::null_mut(), &mut raw);
        if result != 0 || raw.is_null() {
            CoTaskMemFree(raw.cast());
            return Err(AppError::Storage(
                "the current user's LocalAppData directory is unavailable".into(),
            ));
        }
        let mut length = 0;
        while *raw.add(length) != 0 {
            length += 1;
        }
        let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(raw, length)));
        CoTaskMemFree(raw.cast());
        path
    };
    let config: tauri::Config = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|_| AppError::Storage("the application identifier is unavailable".into()))?;
    Ok(local.join(config.identifier))
}

#[cfg(not(windows))]
fn bootstrap_data_dir() -> AppResult<PathBuf> {
    Err(AppError::ToolUpdate(
        "installer bootstrap supports Windows x64 only".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InstallerToolPreference;

    #[test]
    fn deferred_bootstrap_and_preference_commit_require_explicit_ordered_arguments() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        for (argument, preference) in [
            ("--auto-update=enabled", InstallerToolPreference::Enabled),
            ("--auto-update=disabled", InstallerToolPreference::Disabled),
        ] {
            assert_eq!(
                parse_args(&args(&[
                    "--installer-bootstrap",
                    argument,
                    "--defer-preferences",
                ]))
                .unwrap(),
                Some(InstallerAction::DeferredBootstrap(preference))
            );
            assert_eq!(
                parse_args(&args(&["--installer-commit-preferences", argument])).unwrap(),
                Some(InstallerAction::CommitPreferences(preference))
            );
        }
        for invalid in [
            vec!["--installer-commit-preferences"],
            vec!["--defer-preferences"],
            vec![
                "--installer-bootstrap",
                "--defer-preferences",
                "--auto-update=enabled",
            ],
            vec![
                "--installer-commit-preferences",
                "--auto-update=enabled",
                "--defer-preferences",
            ],
        ] {
            assert!(parse_args(&args(&invalid)).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn bootstrap_cli_requires_one_explicit_preference_and_leaves_normal_startup_alone() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(parse_args(&args(&[])).unwrap(), None);
        assert_eq!(parse_args(&args(&["--normal-argument"])).unwrap(), None);
        assert_eq!(
            parse_args(&args(&["--installer-bootstrap", "--auto-update=enabled"])).unwrap(),
            Some(InstallerAction::Bootstrap(InstallerToolPreference::Enabled))
        );
        assert_eq!(
            parse_args(&args(&["--installer-bootstrap", "--auto-update=disabled"])).unwrap(),
            Some(InstallerAction::Bootstrap(
                InstallerToolPreference::Disabled
            ))
        );
        for invalid in [
            vec!["--installer-bootstrap"],
            vec!["--auto-update=enabled"],
            vec!["--installer-bootstrap", "--auto-update=undecided"],
            vec!["--installer-bootstrap", "--auto-update=true"],
            vec!["--installer-bootstrap", "--auto-update="],
            vec![
                "--installer-bootstrap",
                "--auto-update=enabled",
                "--auto-update=disabled",
            ],
            vec![
                "--installer-bootstrap",
                "--auto-update=enabled",
                "--data-dir=C:/other",
            ],
        ] {
            assert!(parse_args(&args(&invalid)).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn progress_writes_each_stage_as_a_flushed_line_without_repeating_byte_events() {
        #[derive(Default)]
        struct Writer {
            bytes: Vec<u8>,
            flushes: usize,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }
        let mut output = BootstrapOutput {
            writer: Writer::default(),
            last: None,
        };
        for (stage, tool) in [
            (ToolUpdateStage::Checking, None),
            (ToolUpdateStage::Downloading, Some("yt-dlp")),
            (ToolUpdateStage::Downloading, Some("yt-dlp")),
            (ToolUpdateStage::Downloading, Some("FFmpeg")),
            (ToolUpdateStage::Verifying, None),
            (ToolUpdateStage::Installing, None),
        ] {
            output
                .progress(&ToolUpdateProgress {
                    operation_id: "test".into(),
                    stage,
                    tool_name: tool.map(str::to_owned),
                    downloaded_bytes: 1,
                    total_bytes: 2,
                })
                .unwrap();
        }
        output.line("Required media tools are ready.").unwrap();
        assert_eq!(output.writer.flushes, 6);
        assert_eq!(
            String::from_utf8(output.writer.bytes).unwrap(),
            concat!(
                "Checking existing BITIG tools...\nDownloading yt-dlp...\nDownloading FFmpeg...\n",
                "Verifying SHA-256...\nInstalling verified media tools...\nRequired media tools are ready.\n"
            )
        );
    }

    #[cfg(windows)]
    #[test]
    fn bootstrap_resolves_the_configured_current_user_directory_without_starting_tauri() {
        let path = bootstrap_data_dir().unwrap();
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap(), "app.bitig.desktop");
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[tokio::test]
    async fn headless_reinstall_uses_only_tools_and_preferences_and_preserves_them_on_failure() {
        use crate::{
            downloader::{
                CachedToolManifest, cached_installation_name, executable_name,
                write_cached_manifest,
            },
            storage::read_installer_tool_preference,
        };
        use sha2::{Digest, Sha256};
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let installation = root.join("tools").join(cached_installation_name());
        std::fs::create_dir_all(&installation).unwrap();
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            std::fs::write(installation.join(executable_name(name)), name).unwrap();
        }
        write_cached_manifest(
            &installation,
            &CachedToolManifest {
                schema_version: 1,
                yt_dlp_version: "2026.10.26".into(),
                ffmpeg_version: "9.0.2 essentials build".into(),
                yt_dlp_sha256: format!("{:x}", Sha256::digest(b"yt-dlp")),
                ffmpeg_sha256: format!("{:x}", Sha256::digest(b"ffmpeg")),
                ffprobe_sha256: format!("{:x}", Sha256::digest(b"ffprobe")),
            },
        )
        .await
        .unwrap();
        struct MissingConsole;
        impl Write for MissingConsole {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
        }
        bootstrap_at(
            root.to_path_buf(),
            InstallerAction::Bootstrap(InstallerToolPreference::Enabled),
            MissingConsole,
        )
        .await
        .unwrap();
        assert_eq!(
            read_installer_tool_preference(root).unwrap(),
            Some(InstallerToolPreference::Enabled)
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
        let previous = std::fs::read(root.join("installer-preferences.json")).unwrap();
        bootstrap_at(
            root.to_path_buf(),
            InstallerAction::DeferredBootstrap(InstallerToolPreference::Disabled),
            std::io::sink(),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(root.join("installer-preferences.json")).unwrap(),
            previous
        );
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("installer-preferences.json"))
            .unwrap();
        assert!(
            bootstrap_at(
                root.to_path_buf(),
                InstallerAction::Bootstrap(InstallerToolPreference::Disabled),
                std::io::sink()
            )
            .await
            .is_err()
        );
        drop(guard);
        assert_eq!(
            read_installer_tool_preference(root).unwrap(),
            Some(InstallerToolPreference::Enabled)
        );
        assert_eq!(std::fs::read_dir(root.join("tools")).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(installation.join(executable_name("yt-dlp"))).unwrap(),
            b"yt-dlp"
        );
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[tokio::test]
    async fn final_preference_commit_never_touches_tools_and_preserves_previous_data_on_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("tools"), b"not a directory").unwrap();
        for preference in [
            InstallerToolPreference::Enabled,
            InstallerToolPreference::Disabled,
        ] {
            bootstrap_at(
                root.to_path_buf(),
                InstallerAction::CommitPreferences(preference),
                std::io::sink(),
            )
            .await
            .unwrap();
            assert_eq!(
                crate::storage::read_installer_tool_preference(root).unwrap(),
                Some(preference)
            );
        }
        let previous = std::fs::read(root.join("installer-preferences.json")).unwrap();
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("installer-preferences.json"))
            .unwrap();
        assert!(
            bootstrap_at(
                root.to_path_buf(),
                InstallerAction::CommitPreferences(InstallerToolPreference::Enabled),
                std::io::sink(),
            )
            .await
            .is_err()
        );
        drop(guard);
        assert_eq!(
            std::fs::read(root.join("installer-preferences.json")).unwrap(),
            previous
        );
        assert_eq!(
            std::fs::read(root.join("tools")).unwrap(),
            b"not a directory"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[tokio::test]
    async fn provisioning_failure_never_replaces_an_existing_preference() {
        let directory = tempfile::tempdir().unwrap();
        write_installer_tool_preference(directory.path(), InstallerToolPreference::Enabled)
            .unwrap();
        std::fs::write(directory.path().join("tools"), b"blocked directory").unwrap();
        assert!(
            bootstrap_at(
                directory.path().to_path_buf(),
                InstallerAction::Bootstrap(InstallerToolPreference::Disabled),
                std::io::sink()
            )
            .await
            .is_err()
        );
        assert_eq!(
            crate::storage::read_installer_tool_preference(directory.path()).unwrap(),
            Some(InstallerToolPreference::Enabled)
        );
        assert!(!directory.path().join("library.db").exists());
    }
}
