use serde::Serialize;

use super::MediaKind;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAnalysis {
    pub analysis_id: String,
    pub title: String,
    pub source_platform: String,
    pub source_id: String,
    pub creator: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<u64>,
    pub existing_versions: u32,
    pub options: Vec<DownloadOption>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOption {
    pub id: String,
    pub kind: MediaKind,
    pub resolution: Option<u32>,
    pub fps: Option<f32>,
    pub container: String,
    pub estimated_size: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum DownloadStrategy {
    Video {
        video_format_id: String,
        audio_format_id: Option<String>,
        container: String,
    },
    Audio {
        source_format_id: String,
        container: String,
    },
}

#[derive(Clone, Debug)]
pub struct ResolvedDownload {
    pub source_url: String,
    pub analysis: MediaAnalysis,
    pub option: DownloadOption,
    pub strategy: DownloadStrategy,
}
