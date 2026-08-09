use std::{ffi::OsString, path::PathBuf, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;

use crate::{
    downloader::process::{BoundedProcessRunner, ProcessSpec},
    errors::{AppError, AppResult},
};

#[derive(Debug, Deserialize)]
pub struct ExtractedMedia {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub extractor_key: Option<String>,
    #[serde(default)]
    pub extractor: Option<String>,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub formats: Vec<ExtractedFormat>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExtractedFormat {
    pub format_id: String,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub vcodec: Option<String>,
    #[serde(default)]
    pub acodec: Option<String>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub filesize_approx: Option<u64>,
    #[serde(default)]
    pub tbr: Option<f64>,
    #[serde(default)]
    pub abr: Option<f64>,
}

#[async_trait]
pub trait MediaExtractor: Send + Sync {
    async fn analyze(&self, url: &str) -> AppResult<ExtractedMedia>;
}

#[derive(Clone)]
pub struct YtDlpExtractor {
    executable: PathBuf,
    runner: BoundedProcessRunner,
}

impl YtDlpExtractor {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            runner: BoundedProcessRunner,
        }
    }
}

#[async_trait]
impl MediaExtractor for YtDlpExtractor {
    async fn analyze(&self, url: &str) -> AppResult<ExtractedMedia> {
        let args = analysis_args(url);
        let output = self
            .runner
            .run(ProcessSpec {
                program: self.executable.clone(),
                args,
                timeout: Duration::from_secs(45),
                stdout_limit: 16 * 1024 * 1024,
                stderr_limit: 128 * 1024,
            })
            .await?;

        if !output.success {
            tracing::warn!(
                "yt-dlp analysis failed; subprocess output withheld to protect URL credentials"
            );
            return Err(AppError::Analysis("yt-dlp exited unsuccessfully".into()));
        }
        parse_analysis_output(&output.stdout)
    }
}

fn parse_analysis_output(bytes: &[u8]) -> AppResult<ExtractedMedia> {
    serde_json::from_slice(bytes).map_err(|error| {
        tracing::warn!(%error, "yt-dlp returned invalid JSON");
        AppError::Analysis("invalid metadata response".into())
    })
}

fn analysis_args(url: &str) -> Vec<OsString> {
    [
        "--ignore-config",
        "--dump-single-json",
        "--no-playlist",
        "--skip-download",
        "--no-warnings",
        "--",
        url,
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{analysis_args, parse_analysis_output};

    #[test]
    fn analysis_ignores_user_configuration_and_does_not_select_a_format() {
        let args = analysis_args("https://example.com/media");
        let args: Vec<_> = args.iter().map(|value| value.to_string_lossy()).collect();
        assert_eq!(
            args.first().map(|value| value.as_ref()),
            Some("--ignore-config")
        );
        assert!(
            !args
                .iter()
                .any(|value| value == "-f" || value == "--format")
        );
    }

    #[test]
    fn malformed_or_truncated_json_is_rejected() {
        assert!(parse_analysis_output(br#"{"id":"x""#).is_err());
        assert!(parse_analysis_output(b"not-json").is_err());
        assert!(parse_analysis_output(b"null").is_err());
    }

    #[test]
    fn minimal_valid_json_is_accepted() {
        let media = parse_analysis_output(br#"{"id":"x","title":"Example"}"#).unwrap();
        assert_eq!(media.id, "x");
        assert!(media.formats.is_empty());
    }
}
