use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;
use zip::ZipArchive;

use crate::{
    domain::{ToolUpdateProgress, ToolUpdateResult, ToolUpdateStage},
    downloader::{
        CachedToolManifest, ToolCleanupPlan, ToolPaths, cached_installation_name,
        comparable_version, executable_name, is_strictly_newer, load_cached, plan_tool_cleanup,
        verify_yt_dlp, verify_yt_dlp_version, write_cached_manifest,
    },
    errors::{AppError, AppResult},
    storage::{ensure_real_directory, open_regular_file, reject_linked_directories},
};

use super::ToolStatusService;

const YT_DLP_RELEASE_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const FFMPEG_RELEASE_URL: &str = "https://api.github.com/repos/GyanD/codexffmpeg/releases/latest";
const MAX_YT_DLP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FFMPEG_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FFMPEG_BINARY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct ToolUpdateService {
    tools_root: PathBuf,
    temp_root: PathBuf,
    active_paths: Arc<RwLock<ToolPaths>>,
    status: ToolStatusService,
    update_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

#[derive(Clone, Debug)]
struct VerifiedAsset {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug)]
struct UpdateCatalog {
    yt_dlp_version: String,
    yt_dlp: VerifiedAsset,
    ffmpeg_version: String,
    ffmpeg: VerifiedAsset,
}

struct PreparedInstallation {
    manifest: CachedToolManifest,
    updated_tools: Vec<String>,
}

#[derive(Clone)]
struct ProgressReporter {
    operation_id: String,
    emit: Arc<dyn Fn(ToolUpdateProgress) + Send + Sync>,
}

struct TransferSpec<'a> {
    expected_size: u64,
    maximum_size: u64,
    tool_name: &'a str,
    completed_bytes: u64,
    total_bytes: u64,
    progress: &'a ProgressReporter,
}

trait ToolSource: Send + Sync {
    fn catalog(&self) -> AppResult<UpdateCatalog>;
    fn open_asset(&self, asset: &VerifiedAsset) -> AppResult<Box<dyn Read>>;
}

struct GithubTools;

impl ToolSource for GithubTools {
    fn catalog(&self) -> AppResult<UpdateCatalog> {
        fetch_catalog()
    }

    fn open_asset(&self, asset: &VerifiedAsset) -> AppResult<Box<dyn Read>> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10 * 60)))
            .https_only(true)
            .build();
        let agent: ureq::Agent = config.into();
        let response = agent
            .get(&asset.url)
            .header("User-Agent", concat!("BITIG/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
        Ok(Box::new(response.into_parts().1.into_reader()))
    }
}

impl ProgressReporter {
    fn report(
        &self,
        stage: ToolUpdateStage,
        tool_name: Option<&str>,
        downloaded_bytes: u64,
        total_bytes: u64,
    ) {
        (self.emit)(ToolUpdateProgress {
            operation_id: self.operation_id.clone(),
            stage,
            tool_name: tool_name.map(str::to_owned),
            downloaded_bytes,
            total_bytes,
        });
    }
}

impl ToolUpdateService {
    pub(crate) async fn provision_required_tools(
        tools_root: PathBuf,
        temp_root: PathBuf,
        on_progress: impl Fn(ToolUpdateProgress) + Send + Sync + 'static,
    ) -> AppResult<ToolPaths> {
        let progress = ProgressReporter {
            operation_id: uuid::Uuid::now_v7().to_string(),
            emit: Arc::new(on_progress),
        };
        // The owner completes staging and cleanup even if its caller disappears.
        tokio::spawn(Self::provision_with(
            tools_root,
            temp_root,
            Arc::new(GithubTools),
            progress,
        ))
        .await
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?
    }

    async fn provision_with(
        tools_root: PathBuf,
        temp_root: PathBuf,
        source: Arc<dyn ToolSource>,
        progress: ProgressReporter,
    ) -> AppResult<ToolPaths> {
        progress.report(ToolUpdateStage::Checking, None, 0, 0);
        let active = ToolPaths::discover_managed(&tools_root).await?;
        if let Some(paths) = &active
            && verify_yt_dlp(&paths.yt_dlp).await.is_ok()
        {
            return Ok(paths.clone());
        }
        let catalog_source = source.clone();
        let catalog = tokio::task::spawn_blocking(move || catalog_source.catalog())
            .await
            .map_err(|error| AppError::ToolUpdate(error.to_string()))??;
        let update_ffmpeg = active.is_none();
        let (paths, _) = install_catalog_files(
            &tools_root,
            &temp_root,
            active,
            catalog,
            (true, update_ffmpeg),
            progress,
            source,
        )
        .await?;
        Ok(paths)
    }

    pub fn new(
        tools_root: PathBuf,
        temp_root: PathBuf,
        active_paths: ToolPaths,
        status: ToolStatusService,
    ) -> Self {
        Self {
            tools_root,
            temp_root,
            active_paths: Arc::new(RwLock::new(active_paths)),
            status,
            update_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.update_lock.try_lock().is_err()
    }

    pub(crate) async fn inspect_tool_cleanup(
        &self,
        inventory: Vec<ToolPaths>,
    ) -> Option<ToolCleanupPlan> {
        let Ok(guard) = Arc::clone(&self.update_lock).try_lock_owned() else {
            tracing::info!("tool cleanup inspection skipped during an update");
            return None;
        };
        let service = self.clone();
        match tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let active = service.active_paths.read()
                .map_err(|_| AppError::ToolUpdate("tool path lock is unavailable".into()))?.clone();
            let plan = plan_tool_cleanup(&service.tools_root, &active, &inventory)?;
            if !plan.deferred.is_empty() {
                tracing::warn!(retained = plan.retained.len(), deferred = plan.deferred.len(),
                    "old tool installations retained: legacy BITIG processes do not share lifetime locks");
            }
            Ok::<_, AppError>(plan)
        }).await {
            Ok(Ok(plan)) => Some(plan),
            result => {
                tracing::warn!(error = ?result, "tool cleanup inspection skipped; all installations retained");
                None
            }
        }
    }

    pub async fn install_available(
        &self,
        operation_id: String,
        on_progress: impl Fn(ToolUpdateProgress) + Send + Sync + 'static,
    ) -> AppResult<ToolUpdateResult> {
        if uuid::Uuid::parse_str(&operation_id).is_err() {
            return Err(AppError::Validation(
                "invalid tool update operation ID".into(),
            ));
        }
        let progress = ProgressReporter {
            operation_id,
            emit: Arc::new(on_progress),
        };
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            return Err(AppError::ToolUpdate(
                "automatic tool updates support Windows x64 only".into(),
            ));
        }

        self.install_catalog(fetch_catalog, progress).await
    }

    async fn install_catalog(
        &self,
        catalog: impl FnOnce() -> AppResult<UpdateCatalog> + Send + 'static,
        progress: ProgressReporter,
    ) -> AppResult<ToolUpdateResult> {
        let guard = Arc::clone(&self.update_lock)
            .try_lock_owned()
            .map_err(|_| AppError::Conflict("a tool update is already in progress".into()))?;
        let service = self.clone();
        // The owner outlives a cancelled caller and awaits both blocking stages.
        tokio::spawn(async move {
            let _guard = guard;
            debug_assert!(service.is_busy());
            progress.report(ToolUpdateStage::Checking, None, 0, 0);
            let catalog = tokio::task::spawn_blocking(catalog)
                .await
                .map_err(|error| AppError::ToolUpdate(error.to_string()))??;
            service
                .install_catalog_locked(catalog, progress, Arc::new(GithubTools))
                .await
        })
        .await
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?
    }

    async fn install_catalog_locked(
        &self,
        catalog: UpdateCatalog,
        progress: ProgressReporter,
        source: Arc<dyn ToolSource>,
    ) -> AppResult<ToolUpdateResult> {
        let installed = self.status.installed_versions().await;
        let update_yt_dlp = verify_yt_dlp_version(&installed.yt_dlp).is_err()
            || is_strictly_newer(&catalog.yt_dlp_version, &installed.yt_dlp);
        let update_ffmpeg = is_strictly_newer(&catalog.ffmpeg_version, &installed.ffmpeg);
        if !update_yt_dlp && !update_ffmpeg {
            return Ok(ToolUpdateResult {
                updated_tools: Vec::new(),
                restart_required: false,
            });
        }

        let active_paths = self
            .active_paths
            .read()
            .map_err(|_| AppError::ToolUpdate("tool path lock is unavailable".into()))?
            .clone();
        let (paths, updated_tools) = install_catalog_files(
            &self.tools_root,
            &self.temp_root,
            Some(active_paths),
            catalog,
            (update_yt_dlp, update_ffmpeg),
            progress,
            source,
        )
        .await?;
        let versions = paths.versions.clone();
        *self
            .active_paths
            .write()
            .map_err(|_| AppError::ToolUpdate("tool path lock is unavailable".into()))? = paths;
        self.status.set_installed_versions(versions).await;

        Ok(ToolUpdateResult {
            updated_tools,
            restart_required: true,
        })
    }
}

async fn install_catalog_files(
    tools_root: &Path,
    temp_root: &Path,
    active: Option<ToolPaths>,
    catalog: UpdateCatalog,
    updates: (bool, bool),
    progress: ProgressReporter,
    source: Arc<dyn ToolSource>,
) -> AppResult<(ToolPaths, Vec<String>)> {
    reject_linked_directories(tools_root)?;
    ensure_real_directory(temp_root)?;
    let job_root = temp_root.join(uuid::Uuid::now_v7().to_string());
    let staged = job_root.join("installation");
    let total_bytes = (if updates.0 { catalog.yt_dlp.size } else { 0 })
        + (if updates.1 { catalog.ffmpeg.size } else { 0 });
    tokio::fs::create_dir(&job_root).await?;
    let result = async {
        tokio::fs::create_dir(&staged).await?;
        let staged_for_task = staged.clone();
        let progress_for_task = progress.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_installation(
                &staged_for_task,
                active.as_ref(),
                &catalog,
                updates.0,
                updates.1,
                &progress_for_task,
                source.as_ref(),
            )
        })
        .await
        .map_err(|error| AppError::ToolUpdate(error.to_string()))??;
        write_cached_manifest(&staged, &prepared.manifest).await?;
        load_cached(&staged).await?;
        reject_linked_directories(&staged)?;
        ensure_real_directory(tools_root)?;
        progress.report(ToolUpdateStage::Installing, None, total_bytes, total_bytes);
        let final_directory = tools_root.join(cached_installation_name());
        tokio::fs::rename(&staged, &final_directory).await?;
        Ok((load_cached(&final_directory).await?, prepared.updated_tools))
    }
    .await;
    if reject_linked_directories(&job_root).is_ok()
        && let Err(error) = tokio::fs::remove_dir_all(&job_root).await
    {
        tracing::warn!(%error, "tool installation temporary workspace cleanup failed");
    }
    result
}

fn fetch_catalog() -> AppResult<UpdateCatalog> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5 * 60)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    let yt_dlp = fetch_release(&agent, YT_DLP_RELEASE_URL)?;
    let ffmpeg = fetch_release(&agent, FFMPEG_RELEASE_URL)?;
    let yt_dlp_version = validated_version(&yt_dlp.tag_name)?;
    let yt_dlp_asset = select_asset(
        &yt_dlp,
        "yt-dlp.exe",
        &format!("/yt-dlp/yt-dlp/releases/download/{yt_dlp_version}/"),
        MAX_YT_DLP_BYTES,
    )?;
    let (ffmpeg_version, ffmpeg_asset) = ffmpeg_release_candidate(&ffmpeg)?;
    Ok(UpdateCatalog {
        yt_dlp_version,
        yt_dlp: yt_dlp_asset,
        ffmpeg_version,
        ffmpeg: ffmpeg_asset,
    })
}

pub(super) fn fetch_ffmpeg_version(agent: &ureq::Agent) -> AppResult<String> {
    let release = fetch_release(agent, FFMPEG_RELEASE_URL)?;
    ffmpeg_release_candidate(&release).map(|(version, _)| version)
}

fn ffmpeg_release_candidate(release: &GithubRelease) -> AppResult<(String, VerifiedAsset)> {
    let tag = validated_version(&release.tag_name)?;
    if comparable_version(&tag).is_none() {
        return Err(AppError::ToolUpdate(
            "FFmpeg release version cannot be compared".into(),
        ));
    }
    let asset = select_asset(
        release,
        &format!("ffmpeg-{tag}-essentials_build.zip"),
        &format!("/GyanD/codexffmpeg/releases/download/{tag}/"),
        MAX_FFMPEG_ARCHIVE_BYTES,
    )?;
    Ok((format!("{tag} essentials build"), asset))
}

fn fetch_release(agent: &ureq::Agent, endpoint: &str) -> AppResult<GithubRelease> {
    let mut response = agent
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("BITIG/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
    serde_json::from_str(&body).map_err(|error| AppError::ToolUpdate(error.to_string()))
}

fn select_asset(
    release: &GithubRelease,
    expected_name: &str,
    expected_path_prefix: &str,
    maximum_size: u64,
) -> AppResult<VerifiedAsset> {
    if release.prerelease || release.draft {
        return Err(AppError::ToolUpdate(
            "only stable published releases are supported".into(),
        ));
    }
    let matching: Vec<_> = release
        .assets
        .iter()
        .filter(|asset| asset.name == expected_name)
        .collect();
    if matching.len() != 1 {
        return Err(AppError::ToolUpdate(format!(
            "expected one official {expected_name} asset"
        )));
    }
    let asset = matching[0];
    if asset.size == 0 || asset.size > maximum_size {
        return Err(AppError::ToolUpdate(format!(
            "unexpected {expected_name} size"
        )));
    }
    let url = Url::parse(&asset.browser_download_url)
        .map_err(|_| AppError::ToolUpdate("invalid release asset URL".into()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != format!("{expected_path_prefix}{expected_name}")
    {
        return Err(AppError::ToolUpdate(
            "release asset URL is not from the expected repository".into(),
        ));
    }
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|digest| valid_sha256(digest))
        .ok_or_else(|| AppError::ToolUpdate("release asset has no SHA-256 digest".into()))?;
    Ok(VerifiedAsset {
        url: asset.browser_download_url.clone(),
        sha256: sha256.to_owned(),
        size: asset.size,
    })
}

fn prepare_installation(
    staged: &Path,
    active: Option<&ToolPaths>,
    catalog: &UpdateCatalog,
    update_yt_dlp: bool,
    update_ffmpeg: bool,
    progress: &ProgressReporter,
    source: &dyn ToolSource,
) -> AppResult<PreparedInstallation> {
    let yt_dlp_path = staged.join(executable_name("yt-dlp"));
    let ffmpeg_path = staged.join(executable_name("ffmpeg"));
    let ffprobe_path = staged.join(executable_name("ffprobe"));
    let mut updated_tools = Vec::new();
    let total_bytes = (if update_yt_dlp {
        catalog.yt_dlp.size
    } else {
        0
    }) + (if update_ffmpeg {
        catalog.ffmpeg.size
    } else {
        0
    });
    let mut completed_bytes = 0;

    if update_yt_dlp {
        verify_yt_dlp_version(&catalog.yt_dlp_version)?;
        download_verified(
            &catalog.yt_dlp,
            &yt_dlp_path,
            &TransferSpec {
                expected_size: catalog.yt_dlp.size,
                maximum_size: MAX_YT_DLP_BYTES,
                tool_name: "yt-dlp",
                completed_bytes,
                total_bytes,
                progress,
            },
            source,
        )?;
        completed_bytes += catalog.yt_dlp.size;
        updated_tools.push("yt-dlp".into());
    } else {
        std::fs::copy(
            &active.ok_or(AppError::ToolUnavailable)?.yt_dlp,
            &yt_dlp_path,
        )?;
    }

    if update_ffmpeg {
        let archive_path = staged.join("ffmpeg-update.zip");
        download_verified(
            &catalog.ffmpeg,
            &archive_path,
            &TransferSpec {
                expected_size: catalog.ffmpeg.size,
                maximum_size: MAX_FFMPEG_ARCHIVE_BYTES,
                tool_name: "FFmpeg",
                completed_bytes,
                total_bytes,
                progress,
            },
            source,
        )?;
        progress.report(
            ToolUpdateStage::Installing,
            Some("FFmpeg"),
            total_bytes,
            total_bytes,
        );
        extract_ffmpeg(&archive_path, &ffmpeg_path, &ffprobe_path)?;
        std::fs::remove_file(archive_path)?;
        updated_tools.push("ffmpeg".into());
    } else {
        let active = active.ok_or(AppError::ToolUnavailable)?;
        std::fs::copy(&active.ffmpeg, &ffmpeg_path)?;
        std::fs::copy(&active.ffprobe, &ffprobe_path)?;
    }

    progress.report(ToolUpdateStage::Verifying, None, total_bytes, total_bytes);
    let yt_dlp_sha256 = sha256_file_sync(&yt_dlp_path)?;
    let ffmpeg_sha256 = sha256_file_sync(&ffmpeg_path)?;
    let ffprobe_sha256 = sha256_file_sync(&ffprobe_path)?;
    Ok(PreparedInstallation {
        manifest: CachedToolManifest {
            schema_version: 1,
            yt_dlp_version: if update_yt_dlp {
                catalog.yt_dlp_version.clone()
            } else {
                active
                    .ok_or(AppError::ToolUnavailable)?
                    .versions
                    .yt_dlp
                    .clone()
            },
            yt_dlp_sha256,
            ffmpeg_version: if update_ffmpeg {
                catalog.ffmpeg_version.clone()
            } else {
                active
                    .ok_or(AppError::ToolUnavailable)?
                    .versions
                    .ffmpeg
                    .clone()
            },
            ffmpeg_sha256,
            ffprobe_sha256,
        },
        updated_tools,
    })
}

fn download_verified(
    asset: &VerifiedAsset,
    destination: &Path,
    transfer: &TransferSpec<'_>,
    source: &dyn ToolSource,
) -> AppResult<()> {
    let mut reader = source
        .open_asset(asset)?
        .take(transfer.maximum_size.saturating_add(1));
    let mut output = File::create(destination)?;
    let copied = copy_asset_with_progress(&mut reader, &mut output, transfer)?;
    output.flush()?;
    output.sync_all()?;
    transfer.progress.report(
        ToolUpdateStage::Verifying,
        Some(transfer.tool_name),
        transfer.completed_bytes + copied,
        transfer.total_bytes,
    );
    let actual = sha256_file_sync(destination)?;
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        return Err(AppError::ToolUpdate(
            "release asset SHA-256 verification failed".into(),
        ));
    }
    Ok(())
}

fn copy_asset_with_progress(
    reader: &mut impl Read,
    output: &mut impl Write,
    transfer: &TransferSpec<'_>,
) -> AppResult<u64> {
    let mut copied = 0;
    let mut buffer = [0_u8; 1024 * 1024];
    transfer.progress.report(
        ToolUpdateStage::Downloading,
        Some(transfer.tool_name),
        transfer.completed_bytes,
        transfer.total_bytes,
    );
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied += read as u64;
        if copied > transfer.expected_size || copied > transfer.maximum_size {
            return Err(AppError::ToolUpdate(
                "release asset size did not match metadata".into(),
            ));
        }
        output.write_all(&buffer[..read])?;
        transfer.progress.report(
            ToolUpdateStage::Downloading,
            Some(transfer.tool_name),
            transfer.completed_bytes + copied,
            transfer.total_bytes,
        );
    }
    if copied != transfer.expected_size {
        return Err(AppError::ToolUpdate(
            "release asset size did not match metadata".into(),
        ));
    }
    Ok(copied)
}

fn extract_ffmpeg(archive_path: &Path, ffmpeg: &Path, ffprobe: &Path) -> AppResult<()> {
    let mut archive = ZipArchive::new(File::open(archive_path)?)
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
    let mut found_ffmpeg = false;
    let mut found_ffprobe = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
        if entry.enclosed_name().is_none() {
            return Err(AppError::ToolUpdate(
                "FFmpeg archive contains an unsafe path".into(),
            ));
        }
        let name = entry.name().replace('\\', "/");
        let (destination, found) = if name.ends_with("/bin/ffmpeg.exe") {
            (ffmpeg, &mut found_ffmpeg)
        } else if name.ends_with("/bin/ffprobe.exe") {
            (ffprobe, &mut found_ffprobe)
        } else {
            continue;
        };
        if *found || entry.size() == 0 || entry.size() > MAX_FFMPEG_BINARY_BYTES {
            return Err(AppError::ToolUpdate(
                "FFmpeg archive has unexpected binary entries".into(),
            ));
        }
        let mut output = File::create(destination)?;
        let copied = std::io::copy(
            &mut entry.by_ref().take(MAX_FFMPEG_BINARY_BYTES + 1),
            &mut output,
        )?;
        if copied != entry.size() || copied > MAX_FFMPEG_BINARY_BYTES {
            return Err(AppError::ToolUpdate(
                "FFmpeg binary size did not match its archive entry".into(),
            ));
        }
        output.flush()?;
        output.sync_all()?;
        *found = true;
    }
    if !found_ffmpeg || !found_ffprobe {
        return Err(AppError::ToolUpdate(
            "FFmpeg archive is missing ffmpeg.exe or ffprobe.exe".into(),
        ));
    }
    Ok(())
}

fn sha256_file_sync(path: &Path) -> AppResult<String> {
    let mut file = open_regular_file(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validated_version(value: &str) -> AppResult<String> {
    let value = value.trim();
    let value = value.strip_prefix('v').unwrap_or(value);
    (!value.is_empty()
        && comparable_version(value).is_some()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')))
    .then(|| value.to_owned())
    .ok_or_else(|| AppError::ToolUpdate("release has an invalid version".into()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Cursor, Write},
        sync::{Arc, Mutex},
    };

    use super::{
        GithubAsset, GithubRelease, MAX_FFMPEG_ARCHIVE_BYTES, ProgressReporter, ToolPaths,
        ToolStatusService, ToolUpdateService, ToolUpdateStage, TransferSpec, UpdateCatalog,
        VerifiedAsset, cached_installation_name, copy_asset_with_progress, executable_name,
        extract_ffmpeg, ffmpeg_release_candidate, is_strictly_newer, prepare_installation,
        select_asset, validated_version, write_cached_manifest,
    };
    use crate::downloader::InstalledToolVersions;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn release(url: &str, digest: Option<&str>) -> GithubRelease {
        GithubRelease {
            tag_name: "2026.08.19".into(),
            prerelease: false,
            draft: false,
            assets: vec![GithubAsset {
                name: "yt-dlp.exe".into(),
                browser_download_url: url.into(),
                digest: digest.map(str::to_owned),
                size: 1024,
            }],
        }
    }

    fn ffmpeg_release() -> GithubRelease {
        serde_json::from_value(serde_json::json!({
            "tag_name": "9.0.2",
            "prerelease": false,
            "draft": false,
            "assets": [{
                "name": "ffmpeg-9.0.2-essentials_build.zip",
                "browser_download_url": "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
                "digest": format!("sha256:{}", "a".repeat(64)),
                "size": 1024
            }]
        })).unwrap()
    }

    #[test]
    fn ffmpeg_candidate_requires_an_installable_verified_release() {
        let mut release = ffmpeg_release();
        let (version, asset) = ffmpeg_release_candidate(&release).unwrap();
        assert_eq!(version, "9.0.2 essentials build");
        assert_eq!(asset.url, release.assets[0].browser_download_url);
        assert_eq!(asset.sha256, "a".repeat(64));
        assert_eq!(asset.size, 1024);
        release.assets[0].size = MAX_FFMPEG_ARCHIVE_BYTES;
        assert!(ffmpeg_release_candidate(&release).is_ok());

        release.assets.clear();
        let latest = ffmpeg_release_candidate(&release)
            .map(|(version, _)| version)
            .map_err(|_| ());
        let status = super::super::tool_status::checked_status("ffmpeg", "FFmpeg", "9.0.1", latest);
        assert!(matches!(
            status.state,
            crate::domain::ToolUpdateState::CheckFailed
        ));
        assert!(status.latest_version.is_none());
    }

    #[test]
    fn release_metadata_rejects_prereleases_and_drafts_even_with_numeric_tags() {
        for flag in ["prerelease", "draft"] {
            let mut value = serde_json::json!({
                "tag_name": "9.0.2", "prerelease": false, "draft": false,
                "assets": [{ "name": "ffmpeg-9.0.2-essentials_build.zip",
                    "browser_download_url": "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
                    "digest": format!("sha256:{}", "a".repeat(64)), "size": 1024 }]
            });
            value[flag] = true.into();
            let release: GithubRelease = serde_json::from_value(value).unwrap();
            assert!(ffmpeg_release_candidate(&release).is_err(), "{flag}");
        }
    }

    #[test]
    fn ffmpeg_candidate_rejects_missing_ambiguous_or_wrong_assets() {
        let mut release = ffmpeg_release();
        release.assets.clear();
        assert!(ffmpeg_release_candidate(&release).is_err());
        let mut release = ffmpeg_release();
        release.assets.push(release.assets[0].clone());
        assert!(ffmpeg_release_candidate(&release).is_err());
        let mut release = ffmpeg_release();
        release.assets[0].name = "ffmpeg-9.0.2-full_build.zip".into();
        assert!(ffmpeg_release_candidate(&release).is_err());
        for size in [0, MAX_FFMPEG_ARCHIVE_BYTES + 1] {
            let mut release = ffmpeg_release();
            release.assets[0].size = size;
            assert!(ffmpeg_release_candidate(&release).is_err());
        }
    }

    #[test]
    fn ffmpeg_candidate_rejects_untrusted_or_mismatched_download_urls() {
        for url in [
            "http://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
            "https://example.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
            "https://github.com/example/other/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
            "https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.2-essentials_build.zip",
            "https://github.com/GyanD/codexffmpeg/releases/latest/ffmpeg-9.0.2-essentials_build.zip",
            "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/other.zip",
            "/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
        ] {
            let mut release = ffmpeg_release();
            release.assets[0].browser_download_url = url.into();
            assert!(ffmpeg_release_candidate(&release).is_err(), "{url}");
        }
    }

    #[test]
    fn ffmpeg_candidate_rejects_missing_or_invalid_digests() {
        for digest in [
            None,
            Some(String::new()),
            Some(format!("sha512:{}", "a".repeat(64))),
            Some(format!("sha256:{}", "a".repeat(63))),
            Some(format!("sha256:{}", "a".repeat(65))),
            Some(format!("sha256:{}", "g".repeat(64))),
        ] {
            let mut release = ffmpeg_release();
            release.assets[0].digest = digest;
            assert!(ffmpeg_release_candidate(&release).is_err());
        }
    }

    #[test]
    fn ffmpeg_candidate_rejects_invalid_or_unordered_release_tags() {
        for tag in [
            "",
            "<html>failure</html>",
            "--output",
            "9..0",
            "10.0-rc1",
            "N-123456-gabcdef",
            "nightly",
            "18446744073709551616",
        ] {
            let mut release = ffmpeg_release();
            release.tag_name = tag.into();
            assert!(ffmpeg_release_candidate(&release).is_err(), "{tag}");
        }
        let mut release = ffmpeg_release();
        release.tag_name = "9.0.3".into();
        assert!(ffmpeg_release_candidate(&release).is_err());
    }

    #[test]
    fn invalid_release_json_fails_the_metadata_check() {
        for body in [
            "{",
            "null",
            r#"{"tag_name":"9.0.2"}"#,
            r#"{"tag_name":9,"assets":[]}"#,
            r#"{"tag_name":"9.0.2","assets":[{"name":"x","size":"1024"}]}"#,
        ] {
            let latest = serde_json::from_str::<GithubRelease>(body)
                .map_err(|_| ())
                .and_then(|release| {
                    ffmpeg_release_candidate(&release)
                        .map(|(version, _)| version)
                        .map_err(|_| ())
                });
            let status =
                super::super::tool_status::checked_status("ffmpeg", "FFmpeg", "9.0.1", latest);
            assert!(matches!(
                status.state,
                crate::domain::ToolUpdateState::CheckFailed
            ));
        }
    }

    fn catalog(yt_dlp: &str, ffmpeg: &str) -> UpdateCatalog {
        let asset = VerifiedAsset {
            url: "invalid://no-download-expected".into(),
            sha256: "a".repeat(64),
            size: 1024,
        };
        UpdateCatalog {
            yt_dlp_version: yt_dlp.into(),
            yt_dlp: asset.clone(),
            ffmpeg_version: ffmpeg.into(),
            ffmpeg: asset,
        }
    }

    fn tool_paths(directory: &std::path::Path, yt_dlp: &str, ffmpeg: &str) -> ToolPaths {
        ToolPaths {
            yt_dlp: directory.join(executable_name("yt-dlp")),
            ffmpeg: directory.join(executable_name("ffmpeg")),
            ffprobe: directory.join(executable_name("ffprobe")),
            versions: InstalledToolVersions {
                yt_dlp: yt_dlp.into(),
                ffmpeg: ffmpeg.into(),
            },
        }
    }

    fn update_service(directory: &std::path::Path) -> ToolUpdateService {
        let active = tool_paths(directory, "2026.10.26", "9.0.1 essentials build");
        let status = ToolStatusService::new(active.versions.clone());
        ToolUpdateService::new(
            directory.join("tools"),
            directory.join("temp"),
            active,
            status,
        )
    }

    fn quiet_progress() -> ProgressReporter {
        ProgressReporter {
            operation_id: "test-update".into(),
            emit: Arc::new(|_| {}),
        }
    }

    #[tokio::test]
    async fn bootstrap_reuses_verified_tools_offline_without_network_or_new_installations() {
        struct OfflineSource(Arc<std::sync::atomic::AtomicUsize>);
        impl super::ToolSource for OfflineSource {
            fn catalog(&self) -> crate::errors::AppResult<UpdateCatalog> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(crate::errors::AppError::ToolUpdate("offline".into()))
            }
            fn open_asset(
                &self,
                _: &VerifiedAsset,
            ) -> crate::errors::AppResult<Box<dyn std::io::Read>> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(crate::errors::AppError::ToolUpdate("offline".into()))
            }
        }
        let (_directory, service, _) = cleanup_fixture().await;
        let count = std::fs::read_dir(&service.tools_root).unwrap().count();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let paths = ToolUpdateService::provision_with(
            service.tools_root.clone(),
            service.temp_root.clone(),
            Arc::new(OfflineSource(calls.clone())),
            quiet_progress(),
        )
        .await
        .unwrap();
        assert_eq!(paths.versions.yt_dlp, "2026.11.10");
        assert_eq!(paths.versions.ffmpeg, "12.0");
        assert_eq!(paths.ffmpeg.parent(), paths.ffprobe.parent());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_dir(&service.tools_root).unwrap().count(),
            count
        );
        assert!(!service.temp_root.exists());
    }

    struct FixtureSource {
        catalog: UpdateCatalog,
        yt_dlp: Vec<u8>,
        ffmpeg: Vec<u8>,
        requests: Arc<Mutex<Vec<String>>>,
        offline: bool,
    }

    impl super::ToolSource for FixtureSource {
        fn catalog(&self) -> crate::errors::AppResult<UpdateCatalog> {
            self.requests.lock().unwrap().push("catalog".into());
            if self.offline {
                return Err(crate::errors::AppError::ToolUpdate("offline".into()));
            }
            Ok(self.catalog.clone())
        }

        fn open_asset(
            &self,
            asset: &VerifiedAsset,
        ) -> crate::errors::AppResult<Box<dyn std::io::Read>> {
            self.requests.lock().unwrap().push(asset.url.clone());
            let bytes = if asset.url == self.catalog.yt_dlp.url {
                &self.yt_dlp
            } else {
                &self.ffmpeg
            };
            Ok(Box::new(Cursor::new(bytes.clone())))
        }
    }

    fn bootstrap_source() -> FixtureSource {
        use sha2::{Digest, Sha256};
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("ffmpeg", b"ffmpeg".as_slice()),
            ("ffprobe", b"ffprobe".as_slice()),
        ] {
            zip.start_file(
                format!("ffmpeg-9.0.2/bin/{name}.exe"),
                SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let ffmpeg = zip.finish().unwrap().into_inner();
        let yt_dlp = b"yt-dlp".to_vec();
        let mut catalog = catalog("2026.10.26", "9.0.2 essentials build");
        catalog.yt_dlp = VerifiedAsset {
            url: "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/yt-dlp.exe".into(),
            sha256: format!("{:x}", Sha256::digest(&yt_dlp)),
            size: yt_dlp.len() as u64,
        };
        catalog.ffmpeg = VerifiedAsset { url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip".into(),
            sha256: format!("{:x}", Sha256::digest(&ffmpeg)), size: ffmpeg.len() as u64 };
        FixtureSource {
            catalog,
            yt_dlp,
            ffmpeg,
            requests: Arc::new(Mutex::new(Vec::new())),
            offline: false,
        }
    }

    #[tokio::test]
    async fn unsafe_cache_cannot_satisfy_required_bootstrap_offline() {
        let (_directory, service, inventory) = cleanup_fixture().await;
        let mut originals = Vec::new();
        for paths in &inventory {
            let manifest_path = paths.yt_dlp.parent().unwrap().join("manifest.json");
            let mut manifest: crate::downloader::CachedToolManifest =
                serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
            manifest.yt_dlp_version = "2026.10.25".into();
            write_cached_manifest(paths.yt_dlp.parent().unwrap(), &manifest)
                .await
                .unwrap();
            for path in [&paths.yt_dlp, &paths.ffmpeg, &paths.ffprobe, &manifest_path] {
                originals.push((path.clone(), std::fs::read(path).unwrap()));
            }
        }
        let mut source = bootstrap_source();
        source.offline = true;
        let requests = source.requests.clone();
        let result = ToolUpdateService::provision_with(
            service.tools_root.clone(),
            service.temp_root.clone(),
            Arc::new(source),
            quiet_progress(),
        )
        .await;
        assert!(
            result.is_err(),
            "unsafe cache incorrectly satisfied required preparation"
        );
        assert_eq!(*requests.lock().unwrap(), ["catalog"]);
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    async fn unsafe_cached_service(version: &str) -> (tempfile::TempDir, ToolUpdateService) {
        let (directory, service, inventory) = cleanup_fixture().await;
        for paths in inventory {
            let manifest_path = paths.yt_dlp.parent().unwrap().join("manifest.json");
            let mut manifest: crate::downloader::CachedToolManifest =
                serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
            manifest.yt_dlp_version = version.into();
            write_cached_manifest(paths.yt_dlp.parent().unwrap(), &manifest)
                .await
                .unwrap();
        }
        let active = ToolPaths::discover_managed(&service.tools_root)
            .await
            .unwrap()
            .unwrap();
        let status = ToolStatusService::new(active.versions.clone());
        let service = ToolUpdateService::new(service.tools_root, service.temp_root, active, status);
        (directory, service)
    }

    #[tokio::test]
    async fn manual_repair_preserves_old_files_and_blocks_the_old_path_until_restart() {
        use crate::downloader::{MediaExtractor, YtDlpExtractor, verify_yt_dlp};
        for version in ["2026.10.25", "nightly"] {
            for fails in [false, true] {
                let (_directory, service) = unsafe_cached_service(version).await;
                let old = service.active_paths.read().unwrap().clone();
                let count = std::fs::read_dir(&service.tools_root).unwrap().count();
                let originals: Vec<_> = [&old.yt_dlp, &old.ffmpeg, &old.ffprobe]
                    .into_iter()
                    .map(|path| (path.clone(), std::fs::read(path).unwrap()))
                    .collect();
                let manifest_path = old.yt_dlp.parent().unwrap().join("manifest.json");
                let manifest_bytes = std::fs::read(&manifest_path).unwrap();
                let mut source = bootstrap_source();
                if fails {
                    source.catalog.yt_dlp.sha256 = "0".repeat(64);
                }
                let requests = source.requests.clone();
                let result = service
                    .install_catalog_locked(
                        source.catalog.clone(),
                        quiet_progress(),
                        Arc::new(source),
                    )
                    .await;
                if fails {
                    assert!(result.is_err());
                    assert_eq!(service.status.installed_versions().await, old.versions);
                    assert_eq!(
                        std::fs::read_dir(&service.tools_root).unwrap().count(),
                        count
                    );
                } else {
                    let result = result.unwrap();
                    assert_eq!(result.updated_tools, ["yt-dlp"]);
                    assert!(result.restart_required);
                    assert_eq!(
                        service.status.installed_versions().await.yt_dlp,
                        "2026.10.26"
                    );
                    let restarted = ToolPaths::discover_managed(&service.tools_root)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_ne!(restarted.yt_dlp, old.yt_dlp);
                    verify_yt_dlp(&restarted.yt_dlp).await.unwrap();
                    assert_eq!(restarted.versions.ffmpeg, old.versions.ffmpeg);
                }
                assert_eq!(requests.lock().unwrap().len(), 1);
                assert!(requests.lock().unwrap()[0].contains("yt-dlp.exe"));
                for (path, bytes) in originals {
                    assert_eq!(std::fs::read(path).unwrap(), bytes);
                }
                assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_bytes);
                assert!(matches!(
                    YtDlpExtractor::new(old.yt_dlp)
                        .analyze("https://example.invalid/media")
                        .await,
                    Err(crate::errors::AppError::UnsafeYtDlp)
                ));
            }
        }
    }

    #[tokio::test]
    async fn required_bootstrap_repairs_only_unsafe_yt_dlp_and_reuses_verified_ffmpeg() {
        let (_directory, service) = unsafe_cached_service("2026.10.25").await;
        let old = service.active_paths.read().unwrap().clone();
        let source = bootstrap_source();
        let requests = source.requests.clone();
        let installed = ToolUpdateService::provision_with(
            service.tools_root,
            service.temp_root,
            Arc::new(source),
            quiet_progress(),
        )
        .await
        .unwrap();
        assert_eq!(installed.versions.yt_dlp, "2026.10.26");
        assert_eq!(installed.versions.ffmpeg, old.versions.ffmpeg);
        assert_eq!(
            std::fs::read(&installed.ffmpeg).unwrap(),
            std::fs::read(&old.ffmpeg).unwrap()
        );
        assert_eq!(
            std::fs::read(&installed.ffprobe).unwrap(),
            std::fs::read(&old.ffprobe).unwrap()
        );
        assert!(old.yt_dlp.exists());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], "catalog");
        assert!(requests[1].contains("yt-dlp.exe"));
    }

    #[tokio::test]
    async fn bootstrap_downloads_and_atomically_installs_a_complete_verified_group() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let temp = directory.path().join("temp");
        let source = bootstrap_source();
        let requests = source.requests.clone();
        let tools = ToolUpdateService::provision_with(
            root.clone(),
            temp.clone(),
            Arc::new(source),
            quiet_progress(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&tools.yt_dlp).unwrap(), b"yt-dlp");
        assert_eq!(std::fs::read(&tools.ffmpeg).unwrap(), b"ffmpeg");
        assert_eq!(std::fs::read(&tools.ffprobe).unwrap(), b"ffprobe");
        assert_eq!(tools.ffmpeg.parent(), tools.ffprobe.parent());
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
        assert_eq!(
            std::fs::read_dir(tools.yt_dlp.parent().unwrap())
                .unwrap()
                .count(),
            4
        );
        let discovered = ToolPaths::discover_managed(&root).await.unwrap().unwrap();
        assert_eq!(discovered.yt_dlp, tools.yt_dlp);
        assert_eq!(discovered.versions.yt_dlp, "2026.10.26");
        assert_eq!(discovered.versions.ffmpeg, "9.0.2 essentials build");
    }

    #[tokio::test]
    async fn unsafe_release_is_rejected_before_any_asset_transfer() {
        for version in ["2026.10.25", "nightly", "2026.13.01"] {
            let directory = tempfile::tempdir().unwrap();
            let mut source = bootstrap_source();
            source.catalog.yt_dlp_version = version.into();
            let requests = source.requests.clone();
            let result = ToolUpdateService::provision_with(
                directory.path().join("tools"),
                directory.path().join("temp"),
                Arc::new(source),
                quiet_progress(),
            )
            .await;
            assert!(result.is_err(), "unsafe release accepted: {version}");
            assert_eq!(*requests.lock().unwrap(), ["catalog"]);
            assert_eq!(
                std::fs::read_dir(directory.path().join("tools"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }

    #[tokio::test]
    async fn bootstrap_failures_never_publish_partial_tools_and_clean_their_workspace() {
        use sha2::{Digest, Sha256};
        for failure in [
            "offline",
            "yt-dlp digest",
            "FFmpeg digest",
            "short read",
            "oversize",
            "ZIP",
            "manifest",
            "rename",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("tools");
            let temp = directory.path().join("temp");
            let mut source = bootstrap_source();
            match failure {
                "offline" => source.offline = true,
                "yt-dlp digest" => source.catalog.yt_dlp.sha256 = "0".repeat(64),
                "FFmpeg digest" => source.catalog.ffmpeg.sha256 = "0".repeat(64),
                "short read" => {
                    source.yt_dlp.pop();
                }
                "oversize" => source.yt_dlp.push(0),
                "ZIP" => {
                    source.ffmpeg = b"invalid ZIP".to_vec();
                    source.catalog.ffmpeg.size = source.ffmpeg.len() as u64;
                    source.catalog.ffmpeg.sha256 = format!("{:x}", Sha256::digest(&source.ffmpeg));
                }
                _ => {}
            }
            let fault_root = root.clone();
            let fault_temp = temp.clone();
            let progress = ProgressReporter {
                operation_id: failure.into(),
                emit: Arc::new(move |progress| {
                    if failure == "manifest"
                        && matches!(progress.stage, ToolUpdateStage::Verifying)
                        && progress.tool_name.is_none()
                    {
                        let job = std::fs::read_dir(&fault_temp)
                            .unwrap()
                            .next()
                            .unwrap()
                            .unwrap()
                            .path();
                        std::fs::create_dir(job.join("installation/manifest.json")).unwrap();
                    }
                    if failure == "rename"
                        && matches!(progress.stage, ToolUpdateStage::Installing)
                        && progress.tool_name.is_none()
                    {
                        std::fs::remove_dir(&fault_root).unwrap();
                        std::fs::write(&fault_root, b"blocked destination").unwrap();
                    }
                }),
            };
            let result = ToolUpdateService::provision_with(
                root.clone(),
                temp.clone(),
                Arc::new(source),
                progress,
            )
            .await;
            assert!(result.is_err(), "{failure}");
            if root.is_dir() {
                assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "{failure}");
            }
            if temp.exists() {
                assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0, "{failure}");
            }
            assert!(!directory.path().join("library.db").exists());
        }
    }

    #[tokio::test]
    async fn bootstrap_ignores_corrupt_incomplete_and_unsupported_cached_installations() {
        for failure in ["hash", "missing file", "manifest"] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("tools");
            let temp = directory.path().join("temp");
            let original = ToolUpdateService::provision_with(
                root.clone(),
                temp.clone(),
                Arc::new(bootstrap_source()),
                quiet_progress(),
            )
            .await
            .unwrap();
            let old_directory = original.yt_dlp.parent().unwrap();
            match failure {
                "hash" => std::fs::write(&original.ffprobe, b"corrupt").unwrap(),
                "missing file" => std::fs::remove_file(&original.ffmpeg).unwrap(),
                _ => std::fs::write(
                    old_directory.join("manifest.json"),
                    b"{\"schemaVersion\":2}",
                )
                .unwrap(),
            }
            let old_yt_dlp = std::fs::read(&original.yt_dlp).unwrap();
            let source = bootstrap_source();
            let requests = source.requests.clone();
            let installed = ToolUpdateService::provision_with(
                root.clone(),
                temp,
                Arc::new(source),
                quiet_progress(),
            )
            .await
            .unwrap();
            assert_ne!(installed.yt_dlp, original.yt_dlp);
            assert_eq!(requests.lock().unwrap().len(), 3);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
            assert_eq!(std::fs::read(&original.yt_dlp).unwrap(), old_yt_dlp);
        }
    }

    async fn cleanup_fixture() -> (tempfile::TempDir, ToolUpdateService, Vec<ToolPaths>) {
        let directory = tempfile::tempdir().unwrap();
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            std::fs::write(directory.path().join(executable_name(name)), name).unwrap();
        }
        let root = directory.path().join("tools");
        for (yt_dlp, version) in [
            ("2026.09.10", "10.0"),
            ("2026.10.10", "11.0"),
            ("2026.11.10", "12.0"),
        ] {
            let active = tool_paths(directory.path(), yt_dlp, version);
            let installation = root.join(cached_installation_name());
            std::fs::create_dir_all(&installation).unwrap();
            let prepared = prepare_installation(
                &installation,
                Some(&active),
                &catalog(yt_dlp, version),
                false,
                false,
                &quiet_progress(),
                &super::GithubTools,
            )
            .unwrap();
            write_cached_manifest(&installation, &prepared.manifest)
                .await
                .unwrap();
        }
        let (active, inventory) = ToolPaths::discover_with_inventory(&root).await.unwrap();
        let status = ToolStatusService::new(active.versions.clone());
        let service = ToolUpdateService::new(root, directory.path().join("temp"), active, status);
        (directory, service, inventory)
    }

    #[tokio::test]
    async fn cleanup_inspection_skips_updates_and_never_removes_deferred_installations() {
        let (_directory, service, inventory) = cleanup_fixture().await;
        let guard = Arc::clone(&service.update_lock).try_lock_owned().unwrap();
        assert!(
            service
                .inspect_tool_cleanup(inventory.clone())
                .await
                .is_none()
        );
        drop(guard);
        let plan = service
            .inspect_tool_cleanup(inventory.clone())
            .await
            .unwrap();
        assert_eq!(plan.deferred.len(), 1);
        for paths in inventory {
            assert!(paths.yt_dlp.is_file());
            assert!(paths.ffmpeg.is_file());
            assert!(paths.ffprobe.is_file());
        }
        assert!(!service.is_busy());
    }

    #[tokio::test]
    async fn cancelled_inspection_keeps_the_update_lock_until_its_worker_finishes() {
        use std::future::Future;
        let (_directory, service, inventory) = cleanup_fixture().await;
        let paths = Arc::clone(&service.active_paths);
        let (started, running) = tokio::sync::oneshot::channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = paths.write().unwrap();
            started.send(()).unwrap();
            wait.recv().unwrap();
        });
        running.await.unwrap();
        let mut inspection = Box::pin(service.inspect_tool_cleanup(inventory.clone()));
        std::future::poll_fn(|context| {
            assert!(inspection.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(inspection);
        assert!(service.is_busy());
        assert!(
            service
                .inspect_tool_cleanup(inventory.clone())
                .await
                .is_none()
        );
        let result = service
            .install_catalog(
                || panic!("update started during inspection"),
                quiet_progress(),
            )
            .await;
        assert!(matches!(result, Err(crate::errors::AppError::Conflict(_))));
        finish.send(()).unwrap();
        tokio::task::spawn_blocking(move || holder.join().unwrap())
            .await
            .unwrap();
        let completed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.update_lock.lock(),
        )
        .await
        .unwrap();
        drop(completed);
        assert!(!service.is_busy());
        for paths in inventory {
            assert!(paths.ffmpeg.exists());
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_legacy_process_can_use_its_old_path_after_cleanup_inspection() {
        use std::{
            io::{BufRead, BufReader},
            os::windows::{fs::OpenOptionsExt, process::CommandExt},
            process::Stdio,
        };
        let (directory, service, inventory) = cleanup_fixture().await;
        let old = inventory
            .iter()
            .find(|paths| paths.versions.ffmpeg == "10.0")
            .unwrap();
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$oldPath = $env:BITIG_TEST_LEGACY_TOOL_PATH; [Console]::WriteLine('ready'); [Console]::In.ReadLine() | Out-Null; [Console]::WriteLine([IO.File]::ReadAllText($oldPath))"])
            .env("BITIG_TEST_LEGACY_TOOL_PATH", &old.yt_dlp)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .creation_flags(0x0800_0000).spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        let coordination = File::create(directory.path().join("coordination-probe.lock")).unwrap();
        coordination.try_lock().unwrap();
        let binary = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&old.ffmpeg)
            .unwrap();
        let plan = service
            .inspect_tool_cleanup(inventory.clone())
            .await
            .unwrap();
        assert!(
            plan.deferred
                .contains(&old.yt_dlp.parent().unwrap().to_path_buf())
        );
        drop(binary);
        writeln!(child.stdin.take().unwrap(), "continue").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "yt-dlp");
        assert!(child.wait().unwrap().success());
        assert!(old.ffmpeg.exists());
    }

    #[tokio::test]
    async fn concurrent_request_is_rejected_before_metadata_progress_or_disk_work() {
        let directory = tempfile::tempdir().unwrap();
        let service = update_service(directory.path());
        assert!(!service.is_busy());
        let installed = service.status.installed_versions().await;
        let first_service = service.clone();
        let (started, running) = tokio::sync::oneshot::channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let first = tokio::spawn(async move {
            first_service
                .install_catalog(
                    move || {
                        started.send(()).unwrap();
                        wait.recv().unwrap();
                        Ok(catalog("2026.10.26", "9.0.1 essentials build"))
                    },
                    quiet_progress(),
                )
                .await
        });
        running.await.unwrap();
        assert!(service.is_busy());
        assert!(service.clone().is_busy());

        let effects = Arc::new(Mutex::new(Vec::new()));
        let metadata_effects = Arc::clone(&effects);
        let progress_effects = Arc::clone(&effects);
        let second = service
            .clone()
            .install_catalog(
                move || {
                    metadata_effects.lock().unwrap().push("metadata");
                    Ok(catalog("2026.10.26", "9.0.1 essentials build"))
                },
                ProgressReporter {
                    operation_id: "rejected-update".into(),
                    emit: Arc::new(move |_| progress_effects.lock().unwrap().push("progress")),
                },
            )
            .await;

        finish.send(()).unwrap();
        let first_result = first.await.unwrap().unwrap();
        assert!(
            matches!(second, Err(crate::errors::AppError::Conflict(ref message)) if message == "a tool update is already in progress"),
            "{second:?}"
        );
        assert!(effects.lock().unwrap().is_empty());
        assert!(first_result.updated_tools.is_empty());
        assert!(!first_result.restart_required);
        assert!(!service.is_busy());
        assert_eq!(service.status.installed_versions().await, installed);
        assert!(!directory.path().join("tools").exists());
        assert!(!directory.path().join("temp").exists());
        assert!(
            service
                .install_catalog(
                    || Ok(catalog("2026.10.26", "9.0.1 essentials build")),
                    quiet_progress()
                )
                .await
                .is_ok()
        );
        assert!(!service.is_busy());
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[tokio::test]
    async fn public_entry_uses_the_same_lock_before_emitting_progress() {
        let directory = tempfile::tempdir().unwrap();
        let service = update_service(directory.path());
        let guard = Arc::clone(&service.update_lock).try_lock_owned().unwrap();
        let result = service
            .clone()
            .install_available(uuid::Uuid::now_v7().to_string(), |_| {
                panic!("rejected request must not emit progress or start a network request");
            })
            .await;
        drop(guard);
        assert!(
            matches!(result, Err(crate::errors::AppError::Conflict(_))),
            "{result:?}"
        );
        assert!(!service.is_busy());
        assert!(!directory.path().join("tools").exists());
        assert!(!directory.path().join("temp").exists());
    }

    #[tokio::test]
    async fn failures_release_the_lock_and_preserve_installed_tools() {
        let directory = tempfile::tempdir().unwrap();
        let service = update_service(directory.path());
        let original_paths = service.active_paths.read().unwrap().clone();
        for failure in [
            "metadata",
            "metadata-panic",
            "preparation",
            "progress-panic",
        ] {
            let result = service
                .install_catalog(
                    move || match failure {
                        "metadata" => Err(crate::errors::AppError::ToolUpdate(
                            "test metadata error".into(),
                        )),
                        "metadata-panic" => panic!("test blocking failure"),
                        "preparation" => Ok(catalog("2026.10.27", "9.0.1 essentials build")),
                        _ => Ok(catalog("2026.10.26", "9.0.1 essentials build")),
                    },
                    ProgressReporter {
                        operation_id: failure.into(),
                        emit: Arc::new(move |_| {
                            if failure == "progress-panic" {
                                panic!("test owner failure");
                            }
                        }),
                    },
                )
                .await;
            assert!(
                matches!(result, Err(crate::errors::AppError::ToolUpdate(_))),
                "{failure}: {result:?}"
            );
            assert!(!service.is_busy(), "{failure}");
            let paths = service.active_paths.read().unwrap().clone();
            assert_eq!(paths.yt_dlp, original_paths.yt_dlp);
            assert_eq!(paths.ffmpeg, original_paths.ffmpeg);
            assert_eq!(paths.ffprobe, original_paths.ffprobe);
            assert_eq!(
                service.status.installed_versions().await,
                original_paths.versions
            );
            assert!(!service.tools_root.exists());
            if service.temp_root.exists() {
                assert_eq!(std::fs::read_dir(&service.temp_root).unwrap().count(), 0);
            }
            assert!(
                service
                    .install_catalog(
                        || Ok(catalog("2026.10.26", "9.0.1 essentials build")),
                        quiet_progress()
                    )
                    .await
                    .is_ok()
            );
            assert!(!service.is_busy());
        }
    }

    #[tokio::test]
    async fn cancelling_the_caller_keeps_the_lock_until_blocking_work_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let service = update_service(directory.path());
        let (started, running) = tokio::sync::oneshot::channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let marker = directory.path().join("blocking-finished");
        let marker_for_task = marker.clone();
        let first_service = service.clone();
        let caller = tokio::spawn(async move {
            first_service
                .install_catalog(
                    move || {
                        started.send(()).unwrap();
                        wait.recv().unwrap();
                        std::fs::write(marker_for_task, b"completed").unwrap();
                        Ok(catalog("2026.10.26", "9.0.1 essentials build"))
                    },
                    quiet_progress(),
                )
                .await
        });
        running.await.unwrap();
        assert!(service.is_busy());
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        let result = service
            .clone()
            .install_catalog(
                || {
                    panic!("second metadata job started before the cancelled caller's work ended");
                },
                ProgressReporter {
                    operation_id: "rejected-after-cancel".into(),
                    emit: Arc::new(|_| panic!("rejected request emitted progress")),
                },
            )
            .await;
        assert!(
            matches!(result, Err(crate::errors::AppError::Conflict(_))),
            "{result:?}"
        );
        assert!(service.is_busy());
        assert!(!marker.exists());
        finish.send(()).unwrap();
        let completed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.update_lock.lock(),
        )
        .await
        .unwrap();
        drop(completed);
        assert_eq!(std::fs::read(marker).unwrap(), b"completed");
        assert!(!service.is_busy());
        assert!(
            service
                .install_catalog(
                    || Ok(catalog("2026.10.26", "9.0.1 essentials build")),
                    quiet_progress()
                )
                .await
                .is_ok()
        );
        assert!(!service.is_busy());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn simultaneous_requests_allow_only_one_operation() {
        const REQUESTS: usize = 8;
        let directory = tempfile::tempdir().unwrap();
        let service = update_service(directory.path());
        let ready = Arc::new(tokio::sync::Barrier::new(REQUESTS + 1));
        let (started, mut running) = tokio::sync::mpsc::unbounded_channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let wait = Arc::new(Mutex::new(wait));
        let (results, mut completed) = tokio::sync::mpsc::unbounded_channel();
        let mut callers = Vec::new();
        for index in 0..REQUESTS {
            let service = service.clone();
            let ready = Arc::clone(&ready);
            let started = started.clone();
            let wait = Arc::clone(&wait);
            let results = results.clone();
            callers.push(tokio::spawn(async move {
                ready.wait().await;
                let result = service
                    .install_catalog(
                        move || {
                            started.send(index).unwrap();
                            wait.lock().unwrap().recv().unwrap();
                            Ok(catalog("2026.10.26", "9.0.1 essentials build"))
                        },
                        quiet_progress(),
                    )
                    .await;
                results.send(result).unwrap();
            }));
        }
        drop(started);
        drop(results);
        ready.wait().await;
        running.recv().await.unwrap();
        // Channels order the race; the timeout only bounds a broken implementation.
        for _ in 1..REQUESTS {
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completed.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                matches!(result, Err(crate::errors::AppError::Conflict(_))),
                "{result:?}"
            );
        }
        assert!(running.try_recv().is_err());
        assert!(service.is_busy());
        finish.send(()).unwrap();
        assert!(completed.recv().await.unwrap().is_ok());
        for caller in callers {
            caller.await.unwrap();
        }
        assert!(!service.is_busy());
    }

    #[tokio::test]
    async fn no_newer_versions_return_without_downloading_or_staging() {
        for (installed_yt, installed_ff, latest_yt, latest_ff) in [
            (
                "2026.10.26",
                "9.0.1 essentials build",
                "2026.10.26",
                "9.0.1 essentials build",
            ),
            (
                "2026.10.26",
                "9.0.1 essentials build",
                "2026.10.26.0",
                "9.0.1.0 essentials build",
            ),
            (
                "2026.10.27",
                "10.0 essentials build",
                "2026.10.26",
                "9.0.1 essentials build",
            ),
            (
                "2026.10.26",
                "N-123456-gabcdef",
                "2026.10.26",
                "10.0 essentials build",
            ),
            (
                "2026.10.26",
                "9.0.1 essentials build",
                "nightly",
                "10.0-rc1 essentials build",
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let tools_root = directory.path().join("tools");
            let temp_root = directory.path().join("temp");
            let active = tool_paths(
                &directory.path().join("missing-active-files"),
                installed_yt,
                installed_ff,
            );
            let status = ToolStatusService::new(active.versions.clone());
            let service = ToolUpdateService::new(
                tools_root.clone(),
                temp_root.clone(),
                active.clone(),
                status.clone(),
            );
            let result = service
                .install_catalog(
                    move || Ok(catalog(latest_yt, latest_ff)),
                    ProgressReporter {
                        operation_id: "no-update".into(),
                        emit: Arc::new(|event| {
                            assert!(matches!(event.stage, ToolUpdateStage::Checking))
                        }),
                    },
                )
                .await
                .unwrap();
            assert!(result.updated_tools.is_empty());
            assert!(!result.restart_required);
            assert!(!tools_root.exists());
            assert!(!temp_root.exists());
            assert_eq!(status.installed_versions().await, active.versions);
        }
    }

    #[tokio::test]
    async fn advertised_candidate_survives_cached_installation_and_restart() {
        let (ffmpeg_version, ffmpeg_asset) = ffmpeg_release_candidate(&ffmpeg_release()).unwrap();
        let mut catalog = catalog("2026.10.27", &ffmpeg_version);
        catalog.ffmpeg = ffmpeg_asset;
        for (id, installed, latest) in [
            ("yt-dlp", "2026.08.19", catalog.yt_dlp_version.as_str()),
            (
                "ffmpeg",
                "9.0.1 essentials build",
                catalog.ffmpeg_version.as_str(),
            ),
        ] {
            let status =
                super::super::tool_status::checked_status(id, id, installed, Ok(latest.into()));
            assert!(matches!(
                status.state,
                crate::domain::ToolUpdateState::UpdateAvailable
            ));
            assert!(is_strictly_newer(latest, installed));
        }

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("verified-files");
        let staged = directory.path().join("staged");
        let tools_root = directory.path().join("tools");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::create_dir_all(&tools_root).unwrap();
        let active = tool_paths(&source, &catalog.yt_dlp_version, &catalog.ffmpeg_version);
        for path in [&active.yt_dlp, &active.ffmpeg, &active.ffprobe] {
            std::fs::write(path, path.file_name().unwrap().to_string_lossy().as_bytes()).unwrap();
        }
        let progress = ProgressReporter {
            operation_id: "restart-test".into(),
            emit: Arc::new(|_| {}),
        };
        let prepared = prepare_installation(
            &staged,
            Some(&active),
            &catalog,
            false,
            false,
            &progress,
            &super::GithubTools,
        )
        .unwrap();
        write_cached_manifest(&staged, &prepared.manifest)
            .await
            .unwrap();
        let installation = tools_root.join(cached_installation_name());
        std::fs::rename(&staged, &installation).unwrap();

        let restarted = ToolPaths::discover_and_verify(&tools_root).await.unwrap();
        assert_eq!(restarted.versions, active.versions);
        assert_eq!(
            restarted.yt_dlp,
            installation.join(executable_name("yt-dlp"))
        );
        assert_eq!(
            restarted.ffmpeg,
            installation.join(executable_name("ffmpeg"))
        );
        assert_eq!(
            restarted.ffprobe,
            installation.join(executable_name("ffprobe"))
        );
        for (id, version) in [
            ("yt-dlp", &restarted.versions.yt_dlp),
            ("ffmpeg", &restarted.versions.ffmpeg),
        ] {
            let status =
                super::super::tool_status::checked_status(id, id, version, Ok(version.clone()));
            assert!(matches!(
                status.state,
                crate::domain::ToolUpdateState::Current
            ));
        }
        let service = ToolUpdateService::new(
            tools_root,
            directory.path().join("temp"),
            restarted.clone(),
            ToolStatusService::new(restarted.versions),
        );
        let result = service
            .install_catalog(move || Ok(catalog), progress)
            .await
            .unwrap();
        assert!(result.updated_tools.is_empty());
        assert!(!result.restart_required);
    }

    #[test]
    fn asset_selection_requires_expected_repository_and_sha256() {
        let valid = release(
            "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe",
            Some(&format!("sha256:{}", "a".repeat(64))),
        );
        assert!(
            select_asset(
                &valid,
                "yt-dlp.exe",
                "/yt-dlp/yt-dlp/releases/download/2026.08.19/",
                2048
            )
            .is_ok()
        );
        let wrong_repository = release(
            "https://github.com/example/other/releases/download/2026.08.19/yt-dlp.exe",
            Some(&format!("sha256:{}", "a".repeat(64))),
        );
        assert!(
            select_asset(
                &wrong_repository,
                "yt-dlp.exe",
                "/yt-dlp/yt-dlp/releases/download/2026.08.19/",
                2048
            )
            .is_err()
        );
        let missing_digest = release(
            "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe",
            None,
        );
        assert!(
            select_asset(
                &missing_digest,
                "yt-dlp.exe",
                "/yt-dlp/yt-dlp/releases/download/2026.08.19/",
                2048
            )
            .is_err()
        );
    }

    #[test]
    fn asset_selection_rejects_an_unexpected_download_filename() {
        let release = release(
            "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/other.exe",
            Some(&format!("sha256:{}", "a".repeat(64))),
        );
        assert!(
            select_asset(
                &release,
                "yt-dlp.exe",
                "/yt-dlp/yt-dlp/releases/download/2026.08.19/",
                2048
            )
            .is_err()
        );
    }

    #[test]
    fn versions_reject_markup_and_option_like_values() {
        assert_eq!(validated_version("v2026.08.19").unwrap(), "2026.08.19");
        assert!(validated_version("<html>failure</html>").is_err());
        assert!(validated_version("--output").is_err());
    }

    #[test]
    fn asset_transfer_reports_real_bytes_across_two_downloads() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let progress = ProgressReporter {
            operation_id: "test-operation".into(),
            emit: Arc::new(move |event| captured.lock().unwrap().push(event)),
        };
        let mut first_output = Vec::new();
        let mut second_output = Vec::new();
        assert_eq!(
            copy_asset_with_progress(
                &mut Cursor::new(b"abcd"),
                &mut first_output,
                &TransferSpec {
                    expected_size: 4,
                    maximum_size: 64,
                    tool_name: "yt-dlp",
                    completed_bytes: 0,
                    total_bytes: 10,
                    progress: &progress,
                },
            )
            .unwrap(),
            4
        );
        assert_eq!(
            copy_asset_with_progress(
                &mut Cursor::new(b"123456"),
                &mut second_output,
                &TransferSpec {
                    expected_size: 6,
                    maximum_size: 64,
                    tool_name: "FFmpeg",
                    completed_bytes: 4,
                    total_bytes: 10,
                    progress: &progress,
                },
            )
            .unwrap(),
            6
        );
        assert_eq!(first_output, b"abcd");
        assert_eq!(second_output, b"123456");
        let bytes: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event.stage, ToolUpdateStage::Downloading))
            .map(|event| (event.downloaded_bytes, event.total_bytes))
            .collect();
        assert_eq!(bytes, [(0, 10), (4, 10), (4, 10), (10, 10)]);
    }

    #[test]
    fn asset_transfer_rejects_bytes_beyond_release_size() {
        let progress = ProgressReporter {
            operation_id: "test-operation".into(),
            emit: Arc::new(|_| {}),
        };
        let mut output = Vec::new();
        assert!(
            copy_asset_with_progress(
                &mut Cursor::new(b"abc"),
                &mut output,
                &TransferSpec {
                    expected_size: 2,
                    maximum_size: 64,
                    tool_name: "yt-dlp",
                    completed_bytes: 0,
                    total_bytes: 2,
                    progress: &progress,
                },
            )
            .is_err()
        );
        assert!(output.is_empty());
    }

    #[test]
    fn ffmpeg_extraction_selects_only_expected_safe_entries() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("ffmpeg.zip");
        let file = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        archive
            .start_file("ffmpeg-build/bin/ffmpeg.exe", options)
            .unwrap();
        archive.write_all(b"ffmpeg").unwrap();
        archive
            .start_file("ffmpeg-build/bin/ffprobe.exe", options)
            .unwrap();
        archive.write_all(b"ffprobe").unwrap();
        archive
            .start_file("ffmpeg-build/doc/readme.txt", options)
            .unwrap();
        archive.write_all(b"ignored").unwrap();
        archive.finish().unwrap();

        let ffmpeg = directory.path().join("ffmpeg.exe");
        let ffprobe = directory.path().join("ffprobe.exe");
        extract_ffmpeg(&archive_path, &ffmpeg, &ffprobe).unwrap();
        assert_eq!(std::fs::read(ffmpeg).unwrap(), b"ffmpeg");
        assert_eq!(std::fs::read(ffprobe).unwrap(), b"ffprobe");
        assert!(!directory.path().join("readme.txt").exists());
    }

    #[test]
    fn ffmpeg_extraction_rejects_zip_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("unsafe.zip");
        let file = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("../escape.txt", options).unwrap();
        archive.write_all(b"escape").unwrap();
        archive.finish().unwrap();

        assert!(
            extract_ffmpeg(
                &archive_path,
                &directory.path().join("ffmpeg.exe"),
                &directory.path().join("ffprobe.exe")
            )
            .is_err()
        );
        assert!(
            !directory
                .path()
                .parent()
                .unwrap()
                .join("escape.txt")
                .exists()
        );
    }
}
