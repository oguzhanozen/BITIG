use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use sqlx::SqlitePool;
use tokio::sync::RwLock;
use url::Url;
use uuid::Uuid;

use crate::{
    domain::{DownloadOption, DownloadStrategy, MediaAnalysis, MediaKind, ResolvedDownload},
    downloader::{ExtractedFormat, ExtractedMedia, MediaExtractor, YtDlpExtractor},
    errors::{AppError, AppResult},
};

const MAX_ANALYSIS_SESSIONS: usize = 32;

#[derive(Default)]
struct AnalysisStore {
    sessions: HashMap<String, AnalysisSession>,
    order: VecDeque<String>,
}

struct AnalysisSession {
    source_url: String,
    analysis: MediaAnalysis,
    strategies: HashMap<String, DownloadStrategy>,
}

#[derive(Clone)]
pub struct MediaAnalysisService {
    extractor: Arc<dyn MediaExtractor>,
    store: Arc<RwLock<AnalysisStore>>,
    pool: SqlitePool,
}

impl MediaAnalysisService {
    pub fn new(pool: SqlitePool, yt_dlp: std::path::PathBuf) -> Self {
        Self {
            extractor: Arc::new(YtDlpExtractor::new(yt_dlp)),
            store: Arc::new(RwLock::new(AnalysisStore::default())),
            pool,
        }
    }

    pub async fn analyze(&self, input: &str) -> AppResult<MediaAnalysis> {
        let url = validate_url(input)?;
        tracing::info!(host = url.host_str(), "media analysis started");
        let extracted = self.extractor.analyze(url.as_str()).await?;
        let analysis_id = Uuid::now_v7().to_string();
        let (mut analysis, strategies) = normalize(extracted, analysis_id.clone())?;
        let existing_versions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM media WHERE source_platform = ? AND source_id = ?",
        )
        .bind(&analysis.source_platform)
        .bind(&analysis.source_id)
        .fetch_one(&self.pool)
        .await?;
        analysis.existing_versions = u32::try_from(existing_versions).unwrap_or(u32::MAX);

        let mut store = self.store.write().await;
        while store.order.len() >= MAX_ANALYSIS_SESSIONS {
            if let Some(expired) = store.order.pop_front() {
                store.sessions.remove(&expired);
            }
        }
        store.order.push_back(analysis_id.clone());
        store.sessions.insert(
            analysis_id,
            AnalysisSession {
                source_url: url.into(),
                analysis: analysis.clone(),
                strategies,
            },
        );
        tracing::info!(options = analysis.options.len(), "media analysis completed");
        Ok(analysis)
    }

    pub async fn resolve_download(
        &self,
        analysis_id: &str,
        option_id: &str,
    ) -> AppResult<ResolvedDownload> {
        let store = self.store.read().await;
        let session = store
            .sessions
            .get(analysis_id)
            .ok_or_else(|| AppError::Validation("analysis session has expired".into()))?;
        let strategy = session
            .strategies
            .get(option_id)
            .cloned()
            .ok_or_else(|| AppError::Validation("download option has expired".into()))?;
        let option = session
            .analysis
            .options
            .iter()
            .find(|option| option.id == option_id)
            .cloned()
            .ok_or_else(|| AppError::Validation("download option has expired".into()))?;
        Ok(ResolvedDownload {
            source_url: session.source_url.clone(),
            analysis: session.analysis.clone(),
            option,
            strategy,
        })
    }
}

fn validate_url(input: &str) -> AppResult<Url> {
    if input.len() > 4_096 {
        return Err(AppError::InvalidUrl);
    }
    let url = Url::parse(input.trim()).map_err(|_| AppError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::InvalidUrl);
    }
    Ok(url)
}

fn normalize(
    extracted: ExtractedMedia,
    analysis_id: String,
) -> AppResult<(MediaAnalysis, HashMap<String, DownloadStrategy>)> {
    let best_audio = extracted
        .formats
        .iter()
        .filter(|format| has_audio(format) && !has_video(format))
        .max_by(|left, right| compare_quality(left, right))
        .or_else(|| {
            extracted
                .formats
                .iter()
                .filter(|format| has_audio(format))
                .max_by(|left, right| compare_quality(left, right))
        })
        .cloned();

    let mut videos = BTreeMap::<u32, ExtractedFormat>::new();
    for format in extracted.formats.iter().filter(|format| has_video(format)) {
        let Some(height) = positive_u32(format.height) else {
            continue;
        };
        let replace = videos
            .get(&height)
            .is_none_or(|current| compare_video_preference(format, current).is_gt());
        if replace {
            videos.insert(height, format.clone());
        }
    }

    let mut options = Vec::new();
    let mut strategies = HashMap::new();
    for (height, video) in videos {
        let separate_audio = (!has_audio(&video)).then(|| best_audio.clone()).flatten();
        if !has_audio(&video) && separate_audio.is_none() {
            continue;
        }
        let container = merged_container(&video, separate_audio.as_ref());
        let id = Uuid::now_v7().to_string();
        let estimated_size = media_size(&video).and_then(|video_size| {
            separate_audio.as_ref().map_or(Some(video_size), |audio| {
                media_size(audio).and_then(|audio_size| video_size.checked_add(audio_size))
            })
        });
        strategies.insert(
            id.clone(),
            DownloadStrategy::Video {
                video_format_id: video.format_id.clone(),
                audio_format_id: separate_audio.map(|audio| audio.format_id),
                container: container.clone(),
            },
        );
        options.push(DownloadOption {
            id,
            kind: MediaKind::Video,
            resolution: Some(height),
            fps: video.fps.map(|fps| fps as f32),
            container,
            estimated_size,
        });
    }

    if let Some(audio) = best_audio {
        for container in ["m4a", "mp3", "wav"] {
            let id = Uuid::now_v7().to_string();
            strategies.insert(
                id.clone(),
                DownloadStrategy::Audio {
                    source_format_id: audio.format_id.clone(),
                    container: container.into(),
                },
            );
            options.push(DownloadOption {
                id,
                kind: MediaKind::Audio,
                resolution: None,
                fps: None,
                container: container.into(),
                estimated_size: if container == "wav" {
                    None
                } else {
                    media_size(&audio)
                },
            });
        }
    }

    if options.is_empty() {
        return Err(AppError::Analysis(
            "no usable media formats were found".into(),
        ));
    }
    let platform = extracted
        .extractor_key
        .or(extracted.extractor)
        .unwrap_or_else(|| "Unknown".into());
    let duration_ms = extracted.duration.and_then(|duration| {
        duration
            .is_finite()
            .then(|| (duration.max(0.0) * 1_000.0) as u64)
    });
    let analysis = MediaAnalysis {
        analysis_id,
        title: extracted.title,
        source_platform: platform,
        source_id: extracted.id,
        creator: extracted.channel.or(extracted.uploader),
        thumbnail_url: extracted.thumbnail,
        duration_ms,
        existing_versions: 0,
        options,
    };
    Ok((analysis, strategies))
}

fn has_video(format: &ExtractedFormat) -> bool {
    safe_format_id(&format.format_id)
        && format
            .vcodec
            .as_deref()
            .is_some_and(|codec| codec != "none")
}

fn has_audio(format: &ExtractedFormat) -> bool {
    if !safe_format_id(&format.format_id) {
        return false;
    }
    format
        .acodec
        .as_deref()
        .is_some_and(|codec| codec != "none")
        || (format.acodec.is_none()
            && (format
                .vcodec
                .as_deref()
                .is_some_and(|codec| codec != "none")
                || (format.vcodec.as_deref() == Some("none") && format.height.is_none())))
}

fn safe_format_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn positive_u32(value: Option<f64>) -> Option<u32> {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value as u32)
}

fn media_size(format: &ExtractedFormat) -> Option<u64> {
    format.filesize.or(format.filesize_approx)
}

fn compare_quality(left: &ExtractedFormat, right: &ExtractedFormat) -> Ordering {
    left.abr
        .or(left.tbr)
        .partial_cmp(&right.abr.or(right.tbr))
        .unwrap_or(Ordering::Equal)
        .then_with(|| media_size(left).cmp(&media_size(right)))
}

fn compare_video_preference(left: &ExtractedFormat, right: &ExtractedFormat) -> Ordering {
    let left_mp4 = left.ext.as_deref() == Some("mp4");
    let right_mp4 = right.ext.as_deref() == Some("mp4");
    left_mp4
        .cmp(&right_mp4)
        .then_with(|| left.fps.partial_cmp(&right.fps).unwrap_or(Ordering::Equal))
        .then_with(|| left.tbr.partial_cmp(&right.tbr).unwrap_or(Ordering::Equal))
}

fn merged_container(video: &ExtractedFormat, audio: Option<&ExtractedFormat>) -> String {
    let video_ext = video.ext.as_deref().unwrap_or("mkv");
    let audio_ext = audio.and_then(|format| format.ext.as_deref());
    match (video_ext, audio_ext) {
        ("mp4", None | Some("m4a" | "mp4")) => "mp4",
        ("webm", None | Some("webm" | "opus")) => "webm",
        (other, None) => other,
        _ => "mkv",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::{normalize, validate_url};
    use crate::{
        domain::{DownloadStrategy, MediaKind},
        downloader::{ExtractedFormat, ExtractedMedia},
    };

    fn format(
        id: &str,
        ext: &str,
        video: &str,
        audio: &str,
        height: Option<f64>,
    ) -> ExtractedFormat {
        ExtractedFormat {
            format_id: id.into(),
            ext: Some(ext.into()),
            vcodec: Some(video.into()),
            acodec: Some(audio.into()),
            height,
            fps: Some(30.0),
            filesize: Some(1_000),
            filesize_approx: None,
            tbr: Some(500.0),
            abr: Some(128.0),
        }
    }

    #[test]
    fn url_validation_blocks_shell_like_and_credential_urls() {
        assert!(validate_url("https://example.com/watch?v=1&x=2").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("https://user:pass@example.com/video").is_err());
    }

    #[test]
    fn normalization_hides_format_ids_and_groups_video_resolution() {
        let extracted = ExtractedMedia {
            id: "source-1".into(),
            title: "Example".into(),
            extractor_key: Some("Example".into()),
            extractor: None,
            uploader: Some("Creator".into()),
            channel: None,
            thumbnail: None,
            duration: Some(90.0),
            formats: vec![
                format("audio-251", "webm", "none", "opus", None),
                format("video-webm", "webm", "vp9", "none", Some(1080.0)),
                format("video-mp4", "mp4", "avc1", "none", Some(1080.0)),
            ],
        };
        let (analysis, strategies) = normalize(extracted, "analysis".into()).unwrap();
        assert_eq!(
            analysis
                .options
                .iter()
                .filter(|option| option.kind == MediaKind::Video)
                .count(),
            1
        );
        assert_eq!(
            analysis
                .options
                .iter()
                .filter(|option| option.kind == MediaKind::Audio)
                .count(),
            3
        );
        assert!(
            analysis
                .options
                .iter()
                .all(|option| !option.id.contains("251"))
        );
        assert_eq!(strategies.len(), 4);
    }

    #[test]
    fn normalization_accepts_x_audio_when_acodec_is_omitted() {
        let mut audio = format("hls-audio-128000-Audio", "mp4", "none", "none", None);
        audio.acodec = None;
        let mut video = format("http-1280", "mp4", "avc1", "none", Some(720.0));
        video.acodec = None;
        let extracted = ExtractedMedia {
            id: "x-source".into(),
            title: "X video".into(),
            extractor_key: Some("Twitter".into()),
            extractor: None,
            uploader: Some("creator".into()),
            channel: None,
            thumbnail: None,
            duration: Some(30.0),
            formats: vec![audio, video],
        };

        let (analysis, strategies) = normalize(extracted, "analysis".into()).unwrap();
        assert_eq!(
            analysis
                .options
                .iter()
                .filter(|option| option.kind == MediaKind::Video)
                .count(),
            1
        );
        assert_eq!(strategies.len(), 4);
        assert!(strategies.values().any(|strategy| matches!(
            strategy,
            DownloadStrategy::Video {
                audio_format_id: None,
                ..
            }
        )));
    }
}
