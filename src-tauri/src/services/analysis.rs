use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use sqlx::SqlitePool;
use tokio::sync::RwLock;
use url::{Host, Url};
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
    download_url: String,
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
                source_url: sanitize_url_for_storage(&url),
                download_url: url.into(),
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
            download_url: session.download_url.clone(),
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
        || is_local_host(&url)
    {
        return Err(AppError::InvalidUrl);
    }
    Ok(url)
}

fn is_local_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
        }
        Some(Host::Ipv4(address)) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_unspecified()
                || address.is_multicast()
        }
        Some(Host::Ipv6(address)) => {
            let first = address.segments()[0];
            address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
        None => true,
    }
}

fn sanitize_url_for_storage(url: &Url) -> String {
    let retained: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| !is_sensitive_query_parameter(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    let mut sanitized = url.clone();
    sanitized.set_fragment(None);
    sanitized.set_query(None);
    if !retained.is_empty() {
        sanitized
            .query_pairs_mut()
            .extend_pairs(retained.iter().map(|(name, value)| (name, value)));
    }
    sanitized.into()
}

fn is_sensitive_query_parameter(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace('_', "-");
    name.contains("token")
        || name.contains("signature")
        || name.contains("credential")
        || matches!(
            name.as_str(),
            "auth"
                | "authorization"
                | "sig"
                | "key"
                | "api-key"
                | "secret"
                | "client-secret"
                | "password"
                | "passwd"
                | "session"
                | "sessionid"
                | "jwt"
                | "expire"
                | "expires"
                | "policy"
                | "key-pair-id"
                | "x-amz-algorithm"
                | "x-amz-date"
                | "x-amz-expires"
                | "x-amz-signedheaders"
                | "x-goog-algorithm"
                | "x-goog-date"
                | "x-goog-expires"
                | "x-goog-signedheaders"
        )
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
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
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
    use super::{
        AnalysisSession, AnalysisStore, MediaAnalysisService, normalize, safe_format_id,
        sanitize_url_for_storage, validate_url,
    };
    use crate::{
        domain::{DownloadOption, DownloadStrategy, MediaAnalysis, MediaKind},
        downloader::{ExtractedFormat, ExtractedMedia, YtDlpExtractor},
    };
    use sqlx::SqlitePool;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};
    use tokio::sync::RwLock;

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
        assert!(validate_url("ftp://example.com/video").is_err());
        assert!(validate_url("https://localhost/video").is_err());
        assert!(validate_url("http://127.0.0.1/video").is_err());
        assert!(validate_url("http://[::1]/video").is_err());
        assert!(validate_url("https://user:pass@example.com/video").is_err());
        assert!(validate_url(&format!("https://example.com/{}", "x".repeat(4_096))).is_err());
    }

    #[test]
    fn storage_url_removes_secrets_and_fragment_but_keeps_media_identity() {
        let url = validate_url(
            "https://example.com/watch?v=abc&access_token=secret&X-Amz-Signature=signed&credential=user#private",
        )
        .unwrap();
        assert_eq!(
            sanitize_url_for_storage(&url),
            "https://example.com/watch?v=abc"
        );
    }

    #[test]
    fn format_ids_reject_option_injection_and_malformed_values() {
        assert!(safe_format_id("137-mp4"));
        assert!(!safe_format_id("--exec"));
        assert!(!safe_format_id("137+140"));
        assert!(!safe_format_id("137 --output C:\\temp\\x"));
        assert!(!safe_format_id(&"a".repeat(81)));
    }

    #[tokio::test]
    async fn download_resolution_rejects_unknown_or_malformed_option_ids() {
        let option = DownloadOption {
            id: "opaque-option-id".into(),
            kind: MediaKind::Video,
            resolution: Some(720),
            fps: Some(30.0),
            container: "mp4".into(),
            estimated_size: None,
        };
        let analysis = MediaAnalysis {
            analysis_id: "analysis-id".into(),
            title: "Example".into(),
            source_platform: "Example".into(),
            source_id: "source-id".into(),
            creator: None,
            thumbnail_url: None,
            duration_ms: None,
            existing_versions: 0,
            options: vec![option],
        };
        let strategies = HashMap::from([(
            "opaque-option-id".into(),
            DownloadStrategy::Video {
                video_format_id: "137".into(),
                audio_format_id: None,
                container: "mp4".into(),
            },
        )]);
        let store = AnalysisStore {
            sessions: HashMap::from([(
                "analysis-id".into(),
                AnalysisSession {
                    source_url: "https://example.com/watch?v=1".into(),
                    download_url: "https://example.com/watch?v=1&token=secret".into(),
                    analysis,
                    strategies,
                },
            )]),
            order: Default::default(),
        };
        let service = MediaAnalysisService {
            extractor: Arc::new(YtDlpExtractor::new(PathBuf::from("unused"))),
            store: Arc::new(RwLock::new(store)),
            pool: SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
        };

        assert!(
            service
                .resolve_download("analysis-id", "--exec")
                .await
                .is_err()
        );
        assert!(
            service
                .resolve_download("analysis-id", "missing")
                .await
                .is_err()
        );
        let resolved = service
            .resolve_download("analysis-id", "opaque-option-id")
            .await
            .unwrap();
        assert_eq!(resolved.source_url, "https://example.com/watch?v=1");
        assert!(resolved.download_url.contains("token=secret"));
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
