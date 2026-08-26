use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;
use zip::ZipArchive;

use crate::{
    domain::ToolUpdateResult,
    downloader::{
        CachedToolManifest, InstalledToolVersions, ToolPaths, cached_installation_name,
        executable_name, write_cached_manifest,
    },
    errors::{AppError, AppResult},
};

use super::{ToolStatusService, tool_status::is_at_least};

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
}

#[derive(Clone, Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
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

impl ToolUpdateService {
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
        }
    }

    pub async fn install_available(&self) -> AppResult<ToolUpdateResult> {
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            return Err(AppError::ToolUpdate(
                "automatic tool updates support Windows x64 only".into(),
            ));
        }

        let catalog = tokio::task::spawn_blocking(fetch_catalog)
            .await
            .map_err(|error| AppError::ToolUpdate(error.to_string()))??;
        let installed = self.status.installed_versions().await;
        let update_yt_dlp = !is_at_least(&installed.yt_dlp, &catalog.yt_dlp_version);
        let update_ffmpeg = !is_at_least(&installed.ffmpeg, &catalog.ffmpeg_version);
        if !update_yt_dlp && !update_ffmpeg {
            return Ok(ToolUpdateResult {
                updated_tools: Vec::new(),
                restart_required: false,
            });
        }

        let job_id = uuid::Uuid::now_v7().to_string();
        let job_root = self.temp_root.join(&job_id);
        let staged = job_root.join("installation");
        tokio::fs::create_dir_all(&staged).await?;
        let active_paths = self
            .active_paths
            .read()
            .map_err(|_| AppError::ToolUpdate("tool path lock is unavailable".into()))?
            .clone();
        let staged_for_task = staged.clone();
        let catalog_for_task = catalog.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_installation(
                &staged_for_task,
                &active_paths,
                &installed,
                &catalog_for_task,
                update_yt_dlp,
                update_ffmpeg,
            )
        })
        .await
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?;

        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&job_root).await;
                return Err(error);
            }
        };
        write_cached_manifest(&staged, &prepared.manifest).await?;
        let installation_name = cached_installation_name();
        let final_directory = self.tools_root.join(&installation_name);
        tokio::fs::rename(&staged, &final_directory).await?;
        let _ = tokio::fs::remove_dir_all(&job_root).await;

        let versions = InstalledToolVersions {
            yt_dlp: prepared.manifest.yt_dlp_version.clone(),
            ffmpeg: prepared.manifest.ffmpeg_version.clone(),
        };
        let paths = ToolPaths {
            yt_dlp: final_directory.join(executable_name("yt-dlp")),
            ffmpeg: final_directory.join(executable_name("ffmpeg")),
            ffprobe: final_directory.join(executable_name("ffprobe")),
            versions: versions.clone(),
        };
        *self
            .active_paths
            .write()
            .map_err(|_| AppError::ToolUpdate("tool path lock is unavailable".into()))? = paths;
        self.status.set_installed_versions(versions).await;

        Ok(ToolUpdateResult {
            updated_tools: prepared.updated_tools,
            restart_required: true,
        })
    }
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
    let ffmpeg_tag = validated_version(&ffmpeg.tag_name)?;
    let yt_dlp_asset = select_asset(
        &yt_dlp,
        "yt-dlp.exe",
        &format!("/yt-dlp/yt-dlp/releases/download/{yt_dlp_version}/"),
        MAX_YT_DLP_BYTES,
    )?;
    let ffmpeg_name = format!("ffmpeg-{ffmpeg_tag}-essentials_build.zip");
    let ffmpeg_asset = select_asset(
        &ffmpeg,
        &ffmpeg_name,
        &format!("/GyanD/codexffmpeg/releases/download/{ffmpeg_tag}/"),
        MAX_FFMPEG_ARCHIVE_BYTES,
    )?;
    Ok(UpdateCatalog {
        yt_dlp_version,
        yt_dlp: yt_dlp_asset,
        ffmpeg_version: format!("{ffmpeg_tag} essentials build"),
        ffmpeg: ffmpeg_asset,
    })
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
        || !url.path().starts_with(expected_path_prefix)
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
    active: &ToolPaths,
    installed: &InstalledToolVersions,
    catalog: &UpdateCatalog,
    update_yt_dlp: bool,
    update_ffmpeg: bool,
) -> AppResult<PreparedInstallation> {
    let yt_dlp_path = staged.join(executable_name("yt-dlp"));
    let ffmpeg_path = staged.join(executable_name("ffmpeg"));
    let ffprobe_path = staged.join(executable_name("ffprobe"));
    let mut updated_tools = Vec::new();

    if update_yt_dlp {
        download_verified(&catalog.yt_dlp, &yt_dlp_path, MAX_YT_DLP_BYTES)?;
        updated_tools.push("yt-dlp".into());
    } else {
        std::fs::copy(&active.yt_dlp, &yt_dlp_path)?;
    }

    if update_ffmpeg {
        let archive_path = staged.join("ffmpeg-update.zip");
        download_verified(&catalog.ffmpeg, &archive_path, MAX_FFMPEG_ARCHIVE_BYTES)?;
        extract_ffmpeg(&archive_path, &ffmpeg_path, &ffprobe_path)?;
        std::fs::remove_file(archive_path)?;
        updated_tools.push("ffmpeg".into());
    } else {
        std::fs::copy(&active.ffmpeg, &ffmpeg_path)?;
        std::fs::copy(&active.ffprobe, &ffprobe_path)?;
    }

    let yt_dlp_sha256 = sha256_file_sync(&yt_dlp_path)?;
    let ffmpeg_sha256 = sha256_file_sync(&ffmpeg_path)?;
    let ffprobe_sha256 = sha256_file_sync(&ffprobe_path)?;
    Ok(PreparedInstallation {
        manifest: CachedToolManifest {
            schema_version: 1,
            yt_dlp_version: if update_yt_dlp {
                catalog.yt_dlp_version.clone()
            } else {
                installed.yt_dlp.clone()
            },
            yt_dlp_sha256,
            ffmpeg_version: if update_ffmpeg {
                catalog.ffmpeg_version.clone()
            } else {
                installed.ffmpeg.clone()
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
    maximum_size: u64,
) -> AppResult<()> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10 * 60)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(&asset.url)
        .header("User-Agent", concat!("BITIG/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|error| AppError::ToolUpdate(error.to_string()))?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(maximum_size.saturating_add(1))
        .reader();
    let mut output = File::create(destination)?;
    let copied = std::io::copy(&mut reader, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    if copied != asset.size || copied > maximum_size {
        return Err(AppError::ToolUpdate(
            "release asset size did not match metadata".into(),
        ));
    }
    let actual = sha256_file_sync(destination)?;
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        return Err(AppError::ToolUpdate(
            "release asset SHA-256 verification failed".into(),
        ));
    }
    Ok(())
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
    let mut file = File::open(path)?;
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
    let value = value.trim().trim_start_matches('v');
    (!value.is_empty()
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
    use std::{fs::File, io::Write};

    use super::{GithubAsset, GithubRelease, extract_ffmpeg, select_asset, validated_version};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn release(url: &str, digest: Option<&str>) -> GithubRelease {
        GithubRelease {
            tag_name: "2026.08.19".into(),
            assets: vec![GithubAsset {
                name: "yt-dlp.exe".into(),
                browser_download_url: url.into(),
                digest: digest.map(str::to_owned),
                size: 1024,
            }],
        }
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
    fn versions_reject_markup_and_option_like_values() {
        assert_eq!(validated_version("v2026.08.19").unwrap(), "2026.08.19");
        assert!(validated_version("<html>failure</html>").is_err());
        assert!(validated_version("--output").is_err());
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
