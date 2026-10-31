use std::time::Duration;

use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    domain::{ToolStatusReport, ToolUpdateState, ToolVersionStatus},
    downloader::{
        InstalledToolVersions, comparable_version, is_strictly_newer, verify_yt_dlp_version,
    },
    errors::{AppError, AppResult},
};

use super::tool_updates::fetch_ffmpeg_version;

const YT_DLP_LATEST_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";

#[derive(Clone)]
pub struct ToolStatusService {
    installed: Arc<RwLock<InstalledToolVersions>>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

struct LatestVersions {
    yt_dlp: Result<String, ()>,
    ffmpeg: Result<String, ()>,
}

impl ToolStatusService {
    pub fn new(installed: InstalledToolVersions) -> Self {
        Self {
            installed: Arc::new(RwLock::new(installed)),
        }
    }

    pub(crate) async fn installed_versions(&self) -> InstalledToolVersions {
        self.installed.read().await.clone()
    }

    pub(crate) async fn set_installed_versions(&self, installed: InstalledToolVersions) {
        *self.installed.write().await = installed;
    }

    pub async fn report(&self, check_updates: bool) -> AppResult<ToolStatusReport> {
        let installed = self.installed.read().await.clone();
        if !check_updates {
            return Ok(ToolStatusReport {
                checked_at: None,
                tools: vec![
                    not_checked("yt-dlp", "yt-dlp", &installed.yt_dlp),
                    not_checked("ffmpeg", "FFmpeg", &installed.ffmpeg),
                ],
            });
        }

        let latest = tokio::task::spawn_blocking(check_latest_versions)
            .await
            .map_err(|_| AppError::ToolUnavailable)?;
        Ok(ToolStatusReport {
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            tools: vec![
                checked_status("yt-dlp", "yt-dlp", &installed.yt_dlp, latest.yt_dlp),
                checked_status("ffmpeg", "FFmpeg", &installed.ffmpeg, latest.ffmpeg),
            ],
        })
    }
}

fn not_checked(id: &str, name: &str, installed_version: &str) -> ToolVersionStatus {
    ToolVersionStatus {
        id: id.into(),
        name: name.into(),
        installed_version: installed_version.into(),
        latest_version: None,
        state: ToolUpdateState::NotChecked,
        integrity_verified: true,
        update_supported: true,
        message: Some(
            if id == "yt-dlp" && verify_yt_dlp_version(installed_version).is_err() {
                AppError::UnsafeYtDlp.public_message()
            } else {
                "Check for updates, then install verified releases here.".into()
            },
        ),
    }
}

pub(super) fn checked_status(
    id: &str,
    name: &str,
    installed_version: &str,
    latest: Result<String, ()>,
) -> ToolVersionStatus {
    let latest = latest.and_then(|version| {
        if (comparable_version(installed_version).is_some()
            || (id == "yt-dlp" && verify_yt_dlp_version(&version).is_ok()))
            && comparable_version(&version).is_some()
        {
            Ok(version)
        } else {
            Err(())
        }
    });
    let mut status = match latest {
        Ok(latest_version) => ToolVersionStatus {
            id: id.into(),
            name: name.into(),
            installed_version: installed_version.into(),
            state: if is_strictly_newer(&latest_version, installed_version)
                || (id == "yt-dlp" && comparable_version(installed_version).is_none())
            {
                ToolUpdateState::UpdateAvailable
            } else {
                ToolUpdateState::Current
            },
            latest_version: Some(latest_version),
            integrity_verified: true,
            update_supported: true,
            message: Some("Official release digests are verified before installation.".into()),
        },
        Err(()) => ToolVersionStatus {
            id: id.into(),
            name: name.into(),
            installed_version: installed_version.into(),
            latest_version: None,
            state: ToolUpdateState::CheckFailed,
            integrity_verified: true,
            update_supported: true,
            message: Some(
                "Update information could not be verified. Please try again later.".into(),
            ),
        },
    };
    if id == "yt-dlp" && verify_yt_dlp_version(installed_version).is_err() {
        status.message = Some(AppError::UnsafeYtDlp.public_message());
    }
    status
}

fn check_latest_versions() -> LatestVersions {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    LatestVersions {
        yt_dlp: fetch_yt_dlp(&agent),
        ffmpeg: fetch_ffmpeg_version(&agent).map_err(|_| ()),
    }
}

fn fetch_yt_dlp(agent: &ureq::Agent) -> Result<String, ()> {
    let body = fetch_body(agent, YT_DLP_LATEST_URL)?;
    let release: GithubRelease = serde_json::from_str(&body).map_err(|_| ())?;
    validate_version(&release.tag_name)
}

fn fetch_body(agent: &ureq::Agent, url: &str) -> Result<String, ()> {
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("BITIG/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|_| ())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(|_| ())?;
    Ok(body)
}

fn validate_version(value: &str) -> Result<String, ()> {
    let value = value.trim().trim_start_matches('v');
    (!value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b' ')))
    .then(|| value.to_owned())
    .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::{ToolUpdateState, checked_status, validate_version};

    #[tokio::test]
    async fn unsafe_yt_dlp_status_explains_the_block_and_permits_manual_repair() {
        for version in ["2026.10.25", "nightly"] {
            let service = super::ToolStatusService::new(crate::downloader::InstalledToolVersions {
                yt_dlp: version.into(),
                ffmpeg: "9.0.1".into(),
            });
            let report = service.report(false).await.unwrap();
            assert!(
                report.tools[0]
                    .message
                    .as_ref()
                    .unwrap()
                    .contains("2026.10.26")
            );
            assert!(report.tools[0].update_supported);
            let checked = checked_status("yt-dlp", "yt-dlp", version, Ok("2026.10.26".into()));
            assert!(matches!(checked.state, ToolUpdateState::UpdateAvailable));
            assert!(checked.message.unwrap().contains("2026.10.26"));
            assert!(
                !report.tools[1]
                    .message
                    .as_ref()
                    .unwrap()
                    .contains("2026.10.26")
            );
        }
    }

    #[test]
    fn trailing_zero_components_do_not_advertise_an_update() {
        let status = super::checked_status("ffmpeg", "FFmpeg", "9.0.1", Ok("9.0.1.0".into()));
        assert!(matches!(
            status.state,
            crate::domain::ToolUpdateState::Current
        ));
    }

    #[test]
    fn compares_date_and_semantic_versions() {
        for (installed, latest, expected) in [
            ("2026.07.04", "2026.07.04", ToolUpdateState::Current),
            ("2026.07.04", "2026.08.01", ToolUpdateState::UpdateAvailable),
            ("9.0 essentials build", "9.0", ToolUpdateState::Current),
        ] {
            let status = checked_status("tool", "Tool", installed, Ok(latest.into()));
            assert_eq!(
                std::mem::discriminant(&status.state),
                std::mem::discriminant(&expected)
            );
        }
    }

    #[test]
    fn rejects_unbounded_or_unexpected_version_responses() {
        assert_eq!(validate_version(" v9.0 \n").unwrap(), "9.0");
        assert!(validate_version("<html>failure</html>").is_err());
        assert!(validate_version(&"x".repeat(65)).is_err());
    }

    #[test]
    fn update_states_follow_the_startup_comparison_policy() {
        for (id, installed, latest, expected) in [
            ("ffmpeg", "9.0.1", "9.0.2", "update_available"),
            ("ffmpeg", "9.0.1", "9.0.1", "current"),
            ("ffmpeg", "9.0.1", "8.1.3", "current"),
            ("ffmpeg", "9.0.1", "9.0.1.0", "current"),
            ("ffmpeg", "9.0", "9.0.0", "current"),
            (
                "ffmpeg",
                "9.0.1 essentials build",
                "9.0.2 essentials build",
                "update_available",
            ),
            (
                "ffmpeg",
                "9.0.1 essentials build",
                "9.0.1.0 essentials build",
                "current",
            ),
            ("ffmpeg", "9.0.1", "10.0-rc1", "check_failed"),
            ("ffmpeg", "10.0-rc1", "10.0", "check_failed"),
            ("ffmpeg", "9.0.1", "N-123456-gabcdef", "check_failed"),
            ("ffmpeg", "N-123456-gabcdef", "9.0.2", "check_failed"),
            ("ffmpeg", "9.0.1", "", "check_failed"),
            ("ffmpeg", "", "9.0.2", "check_failed"),
            ("ffmpeg", "9.0.1", "9.18446744073709551616", "check_failed"),
            ("ffmpeg", "18446744073709551616", "9.0.2", "check_failed"),
            ("yt-dlp", "2026.08.19", "2026.09.10", "update_available"),
            ("yt-dlp", "2026.08.19", "2026.08.19", "current"),
            ("yt-dlp", "2026.08.19", "2026.07.01", "current"),
            (
                "yt-dlp",
                "2026.08.19",
                "2026.08.19.123456",
                "update_available",
            ),
            ("yt-dlp", "2026.08.19.123456", "2026.08.19", "current"),
            ("yt-dlp", "2026.08.19", "2026.09.10-nightly", "check_failed"),
            ("yt-dlp", "2026.08.19", "nightly", "check_failed"),
            ("yt-dlp", "invalid", "2026.09.10", "check_failed"),
            ("yt-dlp", "2026.08.19", "2026..09", "check_failed"),
        ] {
            let status = checked_status(id, id, installed, Ok(latest.into()));
            assert_eq!(
                serde_json::to_value(&status.state).unwrap(),
                expected,
                "{installed} -> {latest}"
            );
            assert_eq!(
                super::is_strictly_newer(latest, installed),
                expected == "update_available"
            );
            assert!(status.integrity_verified);
            assert!(status.update_supported);
            if expected == "check_failed" {
                assert!(status.latest_version.is_none());
                assert!(status.message.is_some());
            } else {
                assert_eq!(status.latest_version.as_deref(), Some(latest));
            }
        }
    }

    #[test]
    fn failed_metadata_check_returns_a_safe_message() {
        let status = checked_status("ffmpeg", "FFmpeg", "9.0.1", Err(()));
        assert!(matches!(status.state, ToolUpdateState::CheckFailed));
        assert!(status.latest_version.is_none());
        assert_eq!(
            status.message.as_deref(),
            Some("Update information could not be verified. Please try again later.")
        );
    }

    #[tokio::test]
    async fn offline_report_preserves_not_checked_and_capability_fields() {
        let service = super::ToolStatusService::new(crate::downloader::InstalledToolVersions {
            yt_dlp: "2026.08.19".into(),
            ffmpeg: "9.0.1 essentials build".into(),
        });
        let report = service.report(false).await.unwrap();
        assert!(report.checked_at.is_none());
        assert_eq!(report.tools.len(), 2);
        for tool in report.tools {
            assert!(matches!(tool.state, ToolUpdateState::NotChecked));
            assert!(tool.latest_version.is_none());
            assert!(tool.integrity_verified);
            assert!(tool.update_supported);
        }
    }
}
