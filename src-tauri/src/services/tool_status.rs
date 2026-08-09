use std::time::Duration;

use serde::Deserialize;

use crate::{
    domain::{ToolStatusReport, ToolUpdateState, ToolVersionStatus},
    errors::{AppError, AppResult},
};

const MANIFEST: &str = include_str!("../../binaries/manifest.json");
const YT_DLP_LATEST_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const FFMPEG_LATEST_URL: &str = "https://www.gyan.dev/ffmpeg/builds/release-version";

#[derive(Clone)]
pub struct ToolStatusService {
    yt_dlp_version: String,
    ffmpeg_version: String,
}

#[derive(Deserialize)]
struct ToolManifest {
    artifacts: Vec<ToolArtifact>,
}

#[derive(Deserialize)]
struct ToolArtifact {
    name: String,
    version: String,
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
    pub fn new() -> AppResult<Self> {
        let manifest: ToolManifest = serde_json::from_str(MANIFEST)
            .map_err(|error| AppError::ToolIntegrity(error.to_string()))?;
        let version = |name: &str| {
            manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.name == name)
                .map(|artifact| artifact.version.clone())
                .ok_or_else(|| AppError::ToolIntegrity(name.into()))
        };
        Ok(Self {
            yt_dlp_version: version("yt-dlp")?,
            ffmpeg_version: version("ffmpeg")?,
        })
    }

    pub async fn report(&self, check_updates: bool) -> AppResult<ToolStatusReport> {
        if !check_updates {
            return Ok(ToolStatusReport {
                checked_at: None,
                tools: vec![
                    not_checked("yt-dlp", "yt-dlp", &self.yt_dlp_version),
                    not_checked("ffmpeg", "FFmpeg", &self.ffmpeg_version),
                ],
            });
        }

        let latest = tokio::task::spawn_blocking(check_latest_versions)
            .await
            .map_err(|_| AppError::ToolUnavailable)?;
        Ok(ToolStatusReport {
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            tools: vec![
                checked_status("yt-dlp", "yt-dlp", &self.yt_dlp_version, latest.yt_dlp),
                checked_status("ffmpeg", "FFmpeg", &self.ffmpeg_version, latest.ffmpeg),
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
        update_supported: false,
        message: Some("Updates are installed with verified BITIG releases.".into()),
    }
}

fn checked_status(
    id: &str,
    name: &str,
    installed_version: &str,
    latest: Result<String, ()>,
) -> ToolVersionStatus {
    match latest {
        Ok(latest_version) => ToolVersionStatus {
            id: id.into(),
            name: name.into(),
            installed_version: installed_version.into(),
            state: if is_at_least(installed_version, &latest_version) {
                ToolUpdateState::Current
            } else {
                ToolUpdateState::UpdateAvailable
            },
            latest_version: Some(latest_version),
            integrity_verified: true,
            update_supported: false,
            message: Some("Updates are installed with verified BITIG releases.".into()),
        },
        Err(()) => ToolVersionStatus {
            id: id.into(),
            name: name.into(),
            installed_version: installed_version.into(),
            latest_version: None,
            state: ToolUpdateState::CheckFailed,
            integrity_verified: true,
            update_supported: false,
            message: Some("The update service could not be reached.".into()),
        },
    }
}

fn check_latest_versions() -> LatestVersions {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    LatestVersions {
        yt_dlp: fetch_yt_dlp(&agent),
        ffmpeg: fetch_body(&agent, FFMPEG_LATEST_URL).and_then(|body| validate_version(&body)),
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

fn is_at_least(installed: &str, latest: &str) -> bool {
    version_numbers(installed) >= version_numbers(latest)
}

fn version_numbers(value: &str) -> Vec<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{is_at_least, validate_version};

    #[test]
    fn compares_date_and_semantic_versions() {
        assert!(is_at_least("2026.07.04", "2026.07.04"));
        assert!(!is_at_least("2026.07.04", "2026.08.01"));
        assert!(is_at_least("9.0 essentials build", "9.0"));
    }

    #[test]
    fn rejects_unbounded_or_unexpected_version_responses() {
        assert_eq!(validate_version(" v9.0 \n").unwrap(), "9.0");
        assert!(validate_version("<html>failure</html>").is_err());
        assert!(validate_version(&"x".repeat(65)).is_err());
    }
}
