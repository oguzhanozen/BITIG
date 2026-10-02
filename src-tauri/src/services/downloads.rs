use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore, mpsc};
use uuid::Uuid;

use crate::{
    domain::{
        DownloadJob, DownloadStatus, DownloadStrategy, MediaKind, ResolvedDownload,
        StartDownloadInput,
    },
    downloader::{BoundedProcessRunner, CancellationToken, ProcessSpec, ToolPaths},
    errors::{AppError, AppResult},
    repositories::DownloadRepository,
    storage::AppPaths,
};

use super::{MediaAnalysisService, folders::validate_name};

#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<DownloadManagerInner>,
}

struct DownloadManagerInner {
    app: AppHandle,
    analysis: MediaAnalysisService,
    downloads: DownloadRepository,
    paths: AppPaths,
    runner: BoundedProcessRunner,
    semaphore: Semaphore,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    yt_dlp: PathBuf,
    ffprobe: PathBuf,
    ffmpeg_location: Option<PathBuf>,
}

impl DownloadManager {
    pub fn new(
        app: AppHandle,
        analysis: MediaAnalysisService,
        pool: SqlitePool,
        paths: AppPaths,
        tools: ToolPaths,
    ) -> Self {
        let ffmpeg_location = tools
            .ffmpeg
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf);
        Self {
            inner: Arc::new(DownloadManagerInner {
                app,
                analysis,
                downloads: DownloadRepository::new(pool),
                paths,
                runner: BoundedProcessRunner,
                semaphore: Semaphore::new(2),
                cancellations: Mutex::new(HashMap::new()),
                yt_dlp: tools.yt_dlp,
                ffprobe: tools.ffprobe,
                ffmpeg_location,
            }),
        }
    }

    pub async fn start(&self, input: StartDownloadInput) -> AppResult<DownloadJob> {
        let title = validate_name(&input.title, "media title", 240)?.to_owned();
        if let Some(folder_id) = input.folder_id.as_deref() {
            let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE id = ?")
                .bind(folder_id)
                .fetch_one(self.inner.downloads.pool())
                .await?;
            if exists != 1 {
                return Err(AppError::Validation("folder does not exist".into()));
            }
        }
        let resolved = self
            .inner
            .analysis
            .resolve_download(&input.analysis_id, &input.option_id)
            .await?;
        let id = Uuid::now_v7().to_string();
        let job = self
            .inner
            .downloads
            .create(
                &id,
                &title,
                &resolved.source_url,
                input.folder_id.as_deref(),
            )
            .await?;
        let cancellation = CancellationToken::default();
        self.inner
            .cancellations
            .lock()
            .await
            .insert(id.clone(), cancellation.clone());

        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = manager.run_job(&id, &title, resolved, cancellation).await {
                tracing::error!(job_id = %id, %error, "download job failed");
                if let Ok(failed) = manager.inner.downloads.fail(&id, &error).await {
                    manager.emit(&failed);
                }
            }
            manager.inner.cancellations.lock().await.remove(&id);
        });
        Ok(job)
    }

    pub async fn list(&self) -> AppResult<Vec<DownloadJob>> {
        self.inner.downloads.list().await
    }

    pub async fn get(&self, id: &str) -> AppResult<DownloadJob> {
        self.inner.downloads.get(id).await
    }

    pub async fn cancel(&self, id: &str) -> AppResult<DownloadJob> {
        let token = self.inner.cancellations.lock().await.get(id).cloned();
        if let Some(token) = token {
            token.cancel();
            Ok(self.inner.downloads.get(id).await?)
        } else {
            Err(AppError::Conflict("download is not active".into()))
        }
    }

    async fn run_job(
        &self,
        job_id: &str,
        title: &str,
        resolved: ResolvedDownload,
        cancellation: CancellationToken,
    ) -> AppResult<()> {
        self.transition(job_id, DownloadStatus::Analyzing).await?;
        let permit = self
            .inner
            .semaphore
            .acquire()
            .await
            .map_err(|_| AppError::Download("download manager stopped".into()))?;
        let job_dir = self.inner.paths.temp.join(job_id);
        tokio::fs::create_dir_all(&job_dir).await?;
        self.transition(job_id, DownloadStatus::Downloading).await?;

        let result = self
            .download_to_temp(job_id, &resolved, &job_dir, cancellation.clone())
            .await;
        drop(permit);
        if let Err(error) = result {
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return Err(error);
        }

        self.transition(job_id, DownloadStatus::Processing).await?;
        let output = find_output(&job_dir).await?;
        let thumbnail = find_thumbnail(&job_dir).await?;
        self.transition(job_id, DownloadStatus::Verifying).await?;
        let probe = self
            .probe(&output, resolved.option.kind, cancellation)
            .await?;
        let metadata = tokio::fs::metadata(&output).await?;
        if metadata.len() == 0 {
            return Err(AppError::MediaValidation);
        }

        self.transition(job_id, DownloadStatus::Finalizing).await?;
        let media_id = Uuid::now_v7().to_string();
        let container = strategy_container(&resolved.strategy)?;
        let relative_path = format!("media/{media_id}.{container}");
        let final_path = self.inner.paths.resolve_managed(&relative_path)?;
        tokio::fs::rename(&output, &final_path).await?;

        let stored_thumbnail = if let Some(source) = thumbnail {
            let relative = format!("thumbnails/{media_id}.webp");
            let destination = self.inner.paths.resolve_managed(&relative)?;
            match tokio::fs::rename(source, &destination).await {
                Ok(()) => Some((relative, destination)),
                Err(error) => {
                    tracing::warn!(%error, media_id, "thumbnail could not be finalized");
                    None
                }
            }
        } else {
            None
        };

        let commit = self
            .commit_media(
                job_id,
                &media_id,
                title,
                &relative_path,
                stored_thumbnail
                    .as_ref()
                    .map(|(relative, _)| relative.as_str()),
                &resolved,
                &probe,
                metadata.len(),
            )
            .await;
        if let Err(error) = commit {
            let _ = tokio::fs::remove_file(&final_path).await;
            if let Some((_, thumbnail_path)) = stored_thumbnail {
                let _ = tokio::fs::remove_file(thumbnail_path).await;
            }
            return Err(error);
        }
        let _ = tokio::fs::remove_dir_all(&job_dir).await;
        self.emit(&self.inner.downloads.get(job_id).await?);
        Ok(())
    }

    async fn download_to_temp(
        &self,
        job_id: &str,
        resolved: &ResolvedDownload,
        job_dir: &Path,
        cancellation: CancellationToken,
    ) -> AppResult<()> {
        let mut args = vec![
            OsString::from("--ignore-config"),
            OsString::from("--no-playlist"),
            OsString::from("--no-warnings"),
            OsString::from("--quiet"),
            OsString::from("--progress"),
            OsString::from("--newline"),
            OsString::from("--progress-delta"),
            OsString::from("0.25"),
            OsString::from("--progress-template"),
            OsString::from(
                "download:bitig-progress:%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s",
            ),
            OsString::from("--write-thumbnail"),
            OsString::from("--convert-thumbnails"),
            OsString::from("webp"),
        ];
        if let Some(path) = &self.inner.ffmpeg_location {
            args.push("--ffmpeg-location".into());
            args.push(path.as_os_str().to_owned());
        }
        match &resolved.strategy {
            DownloadStrategy::Video {
                video_format_id,
                audio_format_id,
                container,
            } => {
                let selector = audio_format_id.as_ref().map_or_else(
                    || video_format_id.clone(),
                    |audio| format!("{video_format_id}+{audio}"),
                );
                args.extend([
                    "-f".into(),
                    selector.into(),
                    "--merge-output-format".into(),
                    container.into(),
                ]);
            }
            DownloadStrategy::Audio {
                source_format_id,
                container,
            } => {
                args.extend([
                    "-f".into(),
                    source_format_id.into(),
                    "-x".into(),
                    "--audio-format".into(),
                    container.into(),
                ]);
            }
        }
        args.extend([
            "-o".into(),
            job_dir.join("output.%(ext)s").into_os_string(),
            "--".into(),
            resolved.download_url.clone().into(),
        ]);
        let (line_sender, mut line_receiver) = mpsc::unbounded_channel::<String>();
        let manager = self.clone();
        let progress_job_id = job_id.to_owned();
        let progress_task = tokio::spawn(async move {
            while let Some(line) = line_receiver.recv().await {
                let Some(progress) = parse_progress_line(&line) else {
                    continue;
                };
                match manager
                    .inner
                    .downloads
                    .update_progress(
                        &progress_job_id,
                        progress.ratio,
                        progress.downloaded_bytes,
                        progress.total_bytes,
                        progress.bytes_per_second,
                    )
                    .await
                {
                    Ok(Some(job)) => manager.emit(&job),
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, job_id = %progress_job_id, "download progress could not be stored")
                    }
                }
            }
        });
        let output = self
            .inner
            .runner
            .run_cancellable_streaming(
                ProcessSpec {
                    program: self.inner.yt_dlp.clone(),
                    args,
                    timeout: Duration::from_secs(6 * 60 * 60),
                    stdout_limit: 4 * 1024 * 1024,
                    stderr_limit: 4 * 1024 * 1024,
                },
                cancellation,
                line_sender,
            )
            .await;
        if let Err(error) = progress_task.await {
            tracing::warn!(%error, job_id, "download progress task stopped unexpectedly");
        }
        let output = output?;
        if !output.success {
            tracing::warn!(
                "yt-dlp download failed; subprocess output withheld to protect URL credentials"
            );
            return Err(AppError::Download("yt-dlp exited unsuccessfully".into()));
        }
        Ok(())
    }

    async fn probe(
        &self,
        path: &Path,
        kind: MediaKind,
        cancellation: CancellationToken,
    ) -> AppResult<ProbeResult> {
        let args = [
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height:format=duration",
            "-of",
            "json",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(path.as_os_str().to_owned()))
        .collect();
        let output = self
            .inner
            .runner
            .run_cancellable(
                ProcessSpec {
                    program: self.inner.ffprobe.clone(),
                    args,
                    timeout: Duration::from_secs(30),
                    stdout_limit: 1024 * 1024,
                    stderr_limit: 128 * 1024,
                },
                cancellation,
            )
            .await?;
        if !output.success {
            return Err(AppError::MediaValidation);
        }
        let probe: ProbeResult =
            serde_json::from_slice(&output.stdout).map_err(|_| AppError::MediaValidation)?;
        let expected = match kind {
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
        };
        if !probe
            .streams
            .iter()
            .any(|stream| stream.codec_type == expected)
        {
            return Err(AppError::MediaValidation);
        }
        Ok(probe)
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_media(
        &self,
        job_id: &str,
        media_id: &str,
        title: &str,
        relative_path: &str,
        thumbnail_path: Option<&str>,
        resolved: &ResolvedDownload,
        probe: &ProbeResult,
        file_size: u64,
    ) -> AppResult<()> {
        DownloadStatus::Finalizing.transition(DownloadStatus::Completed)?;
        let dimensions = probe
            .streams
            .iter()
            .find(|stream| stream.codec_type == "video");
        let duration_ms = probe
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| (value * 1_000.0) as i64)
            .or_else(|| {
                resolved
                    .analysis
                    .duration_ms
                    .and_then(|value| i64::try_from(value).ok())
            });
        let file_size = i64::try_from(file_size)
            .map_err(|_| AppError::Storage("media file is too large".into()))?;
        let mut transaction = self.inner.downloads.pool().begin().await?;
        sqlx::query(
            "INSERT INTO media (id, folder_id, title, media_type, source_url, source_platform, source_id, creator, file_path, thumbnail_path, container, width, height, duration_ms, file_size)
             VALUES (?, (SELECT folder_id FROM downloads WHERE id = ?), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(media_id).bind(job_id).bind(title).bind(match resolved.option.kind { MediaKind::Video => "video", MediaKind::Audio => "audio" })
        .bind(&resolved.source_url).bind(&resolved.analysis.source_platform).bind(&resolved.analysis.source_id)
        .bind(&resolved.analysis.creator).bind(relative_path).bind(thumbnail_path).bind(strategy_container(&resolved.strategy)?)
        .bind(dimensions.and_then(|stream| stream.width)).bind(dimensions.and_then(|stream| stream.height))
        .bind(duration_ms).bind(file_size).execute(&mut *transaction).await?;
        let updated = sqlx::query("UPDATE downloads SET status = 'completed', progress = 1, media_id = ?, completed_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'finalizing'")
            .bind(media_id).bind(job_id).execute(&mut *transaction).await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "download state changed during finalization".into(),
            ));
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn transition(&self, id: &str, status: DownloadStatus) -> AppResult<()> {
        let job = self.inner.downloads.transition(id, status).await?;
        self.emit(&job);
        Ok(())
    }

    fn emit(&self, job: &DownloadJob) {
        if let Err(error) = self.inner.app.emit("download://changed", job) {
            tracing::warn!(%error, "download event could not be emitted");
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProbeResult {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: String,
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, PartialEq)]
struct ProgressUpdate {
    ratio: f64,
    downloaded_bytes: i64,
    total_bytes: Option<i64>,
    bytes_per_second: Option<i64>,
}

fn strategy_container(strategy: &DownloadStrategy) -> AppResult<&str> {
    let container = match strategy {
        DownloadStrategy::Video { container, .. } | DownloadStrategy::Audio { container, .. } => {
            container.as_str()
        }
    };
    matches!(container, "mp4" | "webm" | "mkv" | "mp3" | "m4a" | "wav")
        .then_some(container)
        .ok_or_else(|| AppError::Validation("unsupported output container".into()))
}

async fn find_output(directory: &Path) -> AppResult<PathBuf> {
    let mut entries = tokio::fs::read_dir(directory).await?;
    let mut best: Option<(u64, PathBuf)> = None;
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        let path = entry.path();
        let ignored_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|extension| {
                matches!(
                    extension.as_str(),
                    "part" | "ytdl" | "webp" | "jpg" | "jpeg" | "png"
                )
            });
        if !metadata.is_file() || ignored_extension {
            continue;
        }
        if best.as_ref().is_none_or(|(size, _)| metadata.len() > *size) {
            best = Some((metadata.len(), path));
        }
    }
    best.filter(|(size, _)| *size > 0)
        .map(|(_, path)| path)
        .ok_or(AppError::MediaValidation)
}

async fn find_thumbnail(directory: &Path) -> AppResult<Option<PathBuf>> {
    let mut entries = tokio::fs::read_dir(directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if entry.metadata().await?.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("webp"))
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn parse_progress_line(line: &str) -> Option<ProgressUpdate> {
    const PREFIX: &str = "bitig-progress:";
    let values = line.strip_prefix(PREFIX)?;
    let mut values = values.split('|');
    let downloaded_bytes = parse_positive_number(values.next()?)?;
    let exact_total = values.next().and_then(parse_positive_number);
    let estimated_total = values.next().and_then(parse_positive_number);
    let bytes_per_second = values.next().and_then(parse_positive_number);
    let total_bytes = exact_total.or(estimated_total);
    let ratio = total_bytes.filter(|total| *total > 0).map_or(0.0, |total| {
        (downloaded_bytes as f64 / total as f64).clamp(0.0, 0.99)
    });
    Some(ProgressUpdate {
        ratio,
        downloaded_bytes,
        total_bytes,
        bytes_per_second,
    })
}

fn parse_positive_number(value: &str) -> Option<i64> {
    let value = value.trim().parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.0 && value <= i64::MAX as f64).then_some(value as i64)
}

#[cfg(test)]
mod tests {
    use super::{ProgressUpdate, parse_progress_line};

    #[test]
    fn parses_exact_progress() {
        assert_eq!(
            parse_progress_line("bitig-progress:250|1000|NA|50.5"),
            Some(ProgressUpdate {
                ratio: 0.25,
                downloaded_bytes: 250,
                total_bytes: Some(1000),
                bytes_per_second: Some(50),
            })
        );
    }

    #[test]
    fn uses_estimated_total_and_caps_completion() {
        let progress = parse_progress_line("bitig-progress:1200|NA|1000|NA").unwrap();
        assert_eq!(progress.ratio, 0.99);
        assert_eq!(progress.total_bytes, Some(1000));
        assert_eq!(progress.bytes_per_second, None);
    }

    #[test]
    fn ignores_unrelated_output() {
        assert_eq!(parse_progress_line("[download] 42%"), None);
    }
}
