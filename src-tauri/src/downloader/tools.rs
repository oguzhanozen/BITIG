use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::{
    errors::{AppError, AppResult},
    storage::{ensure_real_directory, is_link, open_regular_file, reject_linked_directories},
};

#[cfg(any(debug_assertions, test))]
const BUNDLED_MANIFEST: &str = include_str!("../../binaries/manifest.json");
const CACHED_MANIFEST: &str = "manifest.json";
pub(crate) const MINIMUM_YT_DLP_VERSION: &str = "2026.10.26";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledToolVersions {
    pub yt_dlp: String,
    pub ffmpeg: String,
}

#[derive(Clone, Debug)]
pub struct ToolPaths {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub versions: InstalledToolVersions,
}

#[derive(Debug, Default)]
pub(crate) struct ToolCleanupPlan {
    pub retained: Vec<PathBuf>,
    pub deferred: Vec<PathBuf>,
}

#[cfg(any(debug_assertions, test))]
#[derive(Deserialize)]
struct BundledToolManifest {
    artifacts: Vec<BundledToolArtifact>,
}

#[cfg(any(debug_assertions, test))]
#[derive(Deserialize)]
struct BundledToolArtifact {
    name: String,
    version: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedToolManifest {
    pub schema_version: u32,
    pub yt_dlp_version: String,
    pub yt_dlp_sha256: String,
    pub ffmpeg_version: String,
    pub ffmpeg_sha256: String,
    pub ffprobe_sha256: String,
}

impl ToolPaths {
    pub(crate) async fn discover_managed(updated_root: &Path) -> AppResult<Option<Self>> {
        ensure_real_directory(updated_root)?;
        Ok(select_cached(
            &load_cached_installations(updated_root).await,
        ))
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Preserve the discovery API; startup also requests its verified inventory."
        )
    )]
    pub async fn discover_and_verify(updated_root: &Path) -> AppResult<Self> {
        Self::discover_with_inventory(updated_root)
            .await
            .map(|(paths, _)| paths)
    }

    pub(crate) async fn discover_with_inventory(
        updated_root: &Path,
    ) -> AppResult<(Self, Vec<Self>)> {
        let inventory = load_cached_installations(updated_root).await;
        let paths = Self::discover_sources(select_cached(&inventory)).await?;
        Ok((paths, inventory))
    }

    #[cfg(not(debug_assertions))]
    async fn discover_sources(cached: Option<Self>) -> AppResult<Self> {
        // Legacy bundled tools must not override the Online Installer's managed installations.
        cached.ok_or(AppError::ToolUnavailable)
    }

    #[cfg(debug_assertions)]
    async fn discover_sources(cached: Option<Self>) -> AppResult<Self> {
        let Some(bundled_directory) = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(ToOwned::to_owned))
        else {
            return cached.ok_or(AppError::ToolUnavailable);
        };
        let (manifest, versions) = match bundled_manifest().and_then(|manifest| {
            let versions = bundled_versions(&manifest)?;
            Ok((manifest, versions))
        }) {
            Ok(bundled) => bundled,
            Err(error) => return cached.ok_or(error),
        };

        let paths = Self {
            yt_dlp: development_tool("BITIG_YTDLP_PATH", "yt-dlp", &bundled_directory),
            ffmpeg: development_ffmpeg(&bundled_directory),
            ffprobe: development_tool("BITIG_FFPROBE_PATH", "ffprobe", &bundled_directory),
            versions,
        };

        if !paths.are_bundled_in(&bundled_directory) {
            // Preserve the development overrides and cache-first fallback outside the bundle.
            return Ok(cached.unwrap_or(paths));
        }

        paths.select_and_verify(cached, &manifest).await
    }

    #[cfg(any(debug_assertions, test))]
    async fn select_and_verify(
        mut self,
        cached: Option<Self>,
        manifest: &BundledToolManifest,
    ) -> AppResult<Self> {
        let Some(cached) = cached else {
            self.verify_bundled(manifest).await?;
            return Ok(self);
        };
        let expected: HashMap<_, _> = manifest
            .artifacts
            .iter()
            .map(|artifact| (artifact.name.as_str(), artifact.sha256.as_str()))
            .collect();
        if verify_path("yt-dlp", &self.yt_dlp, &expected)
            .await
            .is_err()
            || is_strictly_newer(&cached.versions.yt_dlp, &self.versions.yt_dlp)
        {
            self.yt_dlp = cached.yt_dlp;
            self.versions.yt_dlp = cached.versions.yt_dlp;
        }
        if verify_path("ffmpeg", &self.ffmpeg, &expected)
            .await
            .is_err()
            || verify_path("ffprobe", &self.ffprobe, &expected)
                .await
                .is_err()
            || is_strictly_newer(&cached.versions.ffmpeg, &self.versions.ffmpeg)
        {
            self.ffmpeg = cached.ffmpeg;
            self.ffprobe = cached.ffprobe;
            self.versions.ffmpeg = cached.versions.ffmpeg;
        }
        Ok(self)
    }

    #[cfg(any(debug_assertions, test))]
    async fn verify_bundled(&self, manifest: &BundledToolManifest) -> AppResult<()> {
        let expected: HashMap<_, _> = manifest
            .artifacts
            .iter()
            .map(|artifact| (artifact.name.as_str(), artifact.sha256.as_str()))
            .collect();
        verify_paths(self, &expected).await
    }

    #[cfg(debug_assertions)]
    fn are_bundled_in(&self, directory: &Path) -> bool {
        [&self.yt_dlp, &self.ffmpeg, &self.ffprobe]
            .iter()
            .all(|path| path.parent() == Some(directory))
    }
}

pub(crate) async fn write_cached_manifest(
    directory: &Path,
    manifest: &CachedToolManifest,
) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| AppError::ToolIntegrity(error.to_string()))?;
    tokio::fs::write(directory.join(CACHED_MANIFEST), bytes).await?;
    Ok(())
}

pub(crate) fn cached_installation_name() -> String {
    format!("install-{}", uuid::Uuid::now_v7())
}

#[cfg(test)]
async fn discover_cached(updated_root: &Path) -> Option<ToolPaths> {
    select_cached(&load_cached_installations(updated_root).await)
}

async fn load_cached_installations(updated_root: &Path) -> Vec<ToolPaths> {
    let mut entries = match tokio::fs::read_dir(updated_root).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "updated tool directory could not be read");
            return Vec::new();
        }
    };
    let mut candidates = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_installation_name(&name) || !entry.file_type().await.is_ok_and(|kind| kind.is_dir())
        {
            tracing::warn!(name, "unrecognized tool entry was left untouched");
            continue;
        }
        candidates.push((name, entry.path()));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    let mut verified = Vec::new();
    for (_, directory) in candidates {
        if check_cache_layout(updated_root, &directory).is_err() {
            tracing::warn!("unsafe or incomplete cached tool layout was ignored");
            continue;
        }
        match load_cached(&directory).await {
            Ok(paths) => verified.push(paths),
            Err(error) => tracing::warn!(%error, "cached media tools were ignored"),
        }
    }
    verified
}

fn ranked_cached(
    inventory: &[ToolPaths],
    version: fn(&InstalledToolVersions) -> &str,
) -> Vec<&ToolPaths> {
    let mut ranked: Vec<_> = inventory.iter().collect();
    let prefer = |candidate: &str, installed: &str| {
        is_strictly_newer(candidate, installed)
            || (comparable_version(installed).is_none() && comparable_version(candidate).is_some())
    };
    ranked.sort_by(|left, right| {
        let left_version = version(&left.versions);
        let right_version = version(&right.versions);
        if prefer(left_version, right_version) {
            std::cmp::Ordering::Less
        } else if prefer(right_version, left_version) {
            std::cmp::Ordering::Greater
        } else {
            right.yt_dlp.parent().cmp(&left.yt_dlp.parent())
        }
    });
    ranked
}

fn select_cached(inventory: &[ToolPaths]) -> Option<ToolPaths> {
    let ranked = ranked_cached(inventory, |versions| &versions.yt_dlp);
    let yt_dlp = ranked
        .iter()
        .copied()
        .find(|paths| verify_yt_dlp_version(&paths.versions.yt_dlp).is_ok())
        .or_else(|| ranked.first().copied())?;
    let ffmpeg = ranked_cached(inventory, |versions| &versions.ffmpeg)
        .into_iter()
        .next()?;
    Some(ToolPaths {
        yt_dlp: yt_dlp.yt_dlp.clone(),
        ffmpeg: ffmpeg.ffmpeg.clone(),
        ffprobe: ffmpeg.ffprobe.clone(),
        versions: InstalledToolVersions {
            yt_dlp: yt_dlp.versions.yt_dlp.clone(),
            ffmpeg: ffmpeg.versions.ffmpeg.clone(),
        },
    })
}

pub(crate) fn plan_tool_cleanup(
    root: &Path,
    active: &ToolPaths,
    inventory: &[ToolPaths],
) -> AppResult<ToolCleanupPlan> {
    if !root.is_absolute() || root.file_name() != Some(std::ffi::OsStr::new("tools")) {
        return Err(AppError::UnsafePath);
    }
    let mut retained: HashSet<PathBuf> = [&active.yt_dlp, &active.ffmpeg, &active.ffprobe]
        .into_iter()
        .filter_map(|path| {
            path.parent()
                .filter(|directory| directory.parent() == Some(root))
        })
        .map(Path::to_path_buf)
        .collect();
    for version in [
        (|versions: &InstalledToolVersions| versions.yt_dlp.as_str())
            as fn(&InstalledToolVersions) -> &str,
        |versions: &InstalledToolVersions| versions.ffmpeg.as_str(),
    ] {
        for paths in ranked_cached(inventory, version).into_iter().take(2) {
            if let Some(directory) = paths.yt_dlp.parent() {
                retained.insert(directory.to_path_buf());
            }
        }
    }
    let candidates: Vec<_> = inventory
        .iter()
        .filter_map(|paths| paths.yt_dlp.parent())
        .filter(|directory| !retained.contains(*directory))
        .collect();
    let mut deferred = Vec::new();
    if !candidates.is_empty() {
        for ancestor in root.ancestors() {
            let metadata = std::fs::symlink_metadata(ancestor)?;
            if !metadata.is_dir() || is_link(&metadata) {
                return Err(AppError::UnsafePath);
            }
        }
        let active_directories = [&active.yt_dlp, &active.ffmpeg, &active.ffprobe]
            .into_iter()
            .map(|path| {
                path.parent()
                    .ok_or(AppError::UnsafePath)
                    .and_then(|directory| std::fs::canonicalize(directory).map_err(AppError::from))
            })
            .collect::<AppResult<HashSet<_>>>()?;
        for directory in candidates {
            let eligible = check_cache_layout(root, directory).and_then(|()| {
                Ok(!active_directories.contains(&std::fs::canonicalize(directory)?))
            });
            match eligible {
                Ok(true) => deferred.push(directory.to_path_buf()),
                other => {
                    if let Err(error) = other {
                        tracing::warn!(%error, "tool installation ownership could not be verified; retained");
                    }
                    retained.insert(directory.to_path_buf());
                }
            }
        }
    }
    let mut retained: Vec<_> = retained.into_iter().collect();
    retained.sort();
    deferred.sort();
    // Discovery snapshots guide retention, never authorize deletion across legacy processes.
    Ok(ToolCleanupPlan { retained, deferred })
}

fn check_cache_layout(root: &Path, directory: &Path) -> AppResult<()> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(AppError::UnsafePath)?;
    let owned_name = name
        .strip_prefix("install-")
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .is_some_and(|uuid| {
            uuid.get_version_num() == 7
                && uuid.get_variant() == uuid::Variant::RFC4122
                && name == format!("install-{uuid}")
        });
    let metadata = std::fs::symlink_metadata(directory)?;
    if !owned_name || directory.parent() != Some(root) || !metadata.is_dir() || is_link(&metadata) {
        return Err(AppError::UnsafePath);
    }
    let mut expected: HashSet<_> = [
        CACHED_MANIFEST.to_owned(),
        executable_name("yt-dlp"),
        executable_name("ffmpeg"),
        executable_name("ffprobe"),
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() || is_link(&metadata) || !expected.remove(&entry.file_name()) {
            return Err(AppError::UnsafePath);
        }
    }
    if !expected.is_empty() {
        return Err(AppError::ToolIntegrity(
            "incomplete cached installation".into(),
        ));
    }
    Ok(())
}

async fn read_cached_manifest(directory: &Path) -> AppResult<CachedToolManifest> {
    let manifest_path = directory.join(CACHED_MANIFEST);
    let metadata = tokio::fs::symlink_metadata(&manifest_path).await?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > 16 * 1024 {
        return Err(AppError::ToolIntegrity("cached manifest size".into()));
    }
    let mut bytes = Vec::new();
    tokio::fs::File::from_std(open_regular_file(&manifest_path)?)
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > 16 * 1024 {
        return Err(AppError::ToolIntegrity("cached manifest size".into()));
    }
    let manifest: CachedToolManifest = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::ToolIntegrity(error.to_string()))?;
    validate_cached_manifest(&manifest)?;
    Ok(manifest)
}

pub(crate) async fn load_cached(directory: &Path) -> AppResult<ToolPaths> {
    let manifest = read_cached_manifest(directory).await?;
    let paths = ToolPaths {
        yt_dlp: directory.join(executable_name("yt-dlp")),
        ffmpeg: directory.join(executable_name("ffmpeg")),
        ffprobe: directory.join(executable_name("ffprobe")),
        versions: InstalledToolVersions {
            yt_dlp: manifest.yt_dlp_version.clone(),
            ffmpeg: manifest.ffmpeg_version.clone(),
        },
    };
    let expected = HashMap::from([
        ("yt-dlp", manifest.yt_dlp_sha256.as_str()),
        ("ffmpeg", manifest.ffmpeg_sha256.as_str()),
        ("ffprobe", manifest.ffprobe_sha256.as_str()),
    ]);
    verify_paths(&paths, &expected).await?;
    Ok(paths)
}

pub(crate) fn verify_yt_dlp_version(version: &str) -> AppResult<()> {
    if chrono::NaiveDate::parse_from_str(version.strip_prefix('v').unwrap_or(version), "%Y.%m.%d")
        .is_err()
        || comparable_version(version).is_none()
        || is_strictly_newer(MINIMUM_YT_DLP_VERSION, version)
    {
        return Err(AppError::UnsafeYtDlp);
    }
    Ok(())
}

pub(crate) async fn verify_yt_dlp(path: &Path) -> AppResult<()> {
    let verify = async {
        let directory = path.parent().ok_or(AppError::UnsafePath)?;
        let root = directory.parent().ok_or(AppError::UnsafePath)?;
        if !root.is_absolute()
            || root.file_name() != Some(std::ffi::OsStr::new("tools"))
            || path != directory.join(executable_name("yt-dlp"))
        {
            return Err(AppError::UnsafePath);
        }
        reject_linked_directories(directory)?;
        check_cache_layout(root, directory)?;
        let manifest = read_cached_manifest(directory).await?;
        verify_yt_dlp_version(&manifest.yt_dlp_version)?;
        verify_path(
            "yt-dlp",
            path,
            &HashMap::from([("yt-dlp", manifest.yt_dlp_sha256.as_str())]),
        )
        .await
    };
    verify.await.map_err(|_| AppError::UnsafeYtDlp)
}

fn validate_cached_manifest(manifest: &CachedToolManifest) -> AppResult<()> {
    if manifest.schema_version != 1
        || !valid_version(&manifest.yt_dlp_version)
        || !valid_version(&manifest.ffmpeg_version)
        || !valid_hash(&manifest.yt_dlp_sha256)
        || !valid_hash(&manifest.ffmpeg_sha256)
        || !valid_hash(&manifest.ffprobe_sha256)
    {
        return Err(AppError::ToolIntegrity("invalid cached manifest".into()));
    }
    Ok(())
}

async fn verify_paths(paths: &ToolPaths, expected: &HashMap<&str, &str>) -> AppResult<()> {
    for (name, path) in [
        ("yt-dlp", &paths.yt_dlp),
        ("ffmpeg", &paths.ffmpeg),
        ("ffprobe", &paths.ffprobe),
    ] {
        verify_path(name, path, expected).await?;
    }
    Ok(())
}

async fn verify_path(name: &str, path: &Path, expected: &HashMap<&str, &str>) -> AppResult<()> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| AppError::ToolUnavailable)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(AppError::ToolUnavailable);
    }
    let expected_hash = expected
        .get(name)
        .ok_or_else(|| AppError::ToolIntegrity(name.into()))?;
    let actual_hash = sha256_file(path).await?;
    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
        tracing::error!(tool = name, "media tool hash mismatch");
        return Err(AppError::ToolIntegrity(name.into()));
    }
    Ok(())
}

#[cfg(any(debug_assertions, test))]
fn bundled_manifest() -> AppResult<BundledToolManifest> {
    serde_json::from_str(BUNDLED_MANIFEST)
        .map_err(|error| AppError::ToolIntegrity(error.to_string()))
}

#[cfg(any(debug_assertions, test))]
fn bundled_versions(manifest: &BundledToolManifest) -> AppResult<InstalledToolVersions> {
    let version = |name: &str| {
        manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.name == name)
            .map(|artifact| artifact.version.clone())
            .ok_or_else(|| AppError::ToolIntegrity(name.into()))
    };
    Ok(InstalledToolVersions {
        yt_dlp: version("yt-dlp")?,
        ffmpeg: version("ffmpeg")?,
    })
}

pub(crate) async fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = tokio::fs::File::from_std(open_regular_file(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b' '))
}

pub(crate) fn comparable_version(value: &str) -> Option<Vec<u64>> {
    if !valid_version(value) {
        return None;
    }
    let value = value.strip_prefix('v').unwrap_or(value);
    // The pinned FFmpeg release uses this build label; other suffixes are not ordered.
    let value = value.strip_suffix(" essentials build").unwrap_or(value);
    let mut numbers = value
        .split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse::<u64>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    while numbers.len() > 1 && numbers.last() == Some(&0) {
        numbers.pop();
    }
    Some(numbers)
}

pub(crate) fn is_strictly_newer(candidate: &str, installed: &str) -> bool {
    match (comparable_version(candidate), comparable_version(installed)) {
        (Some(candidate), Some(installed)) => candidate > installed,
        _ => false,
    }
}

fn is_installation_name(name: &str) -> bool {
    name.strip_prefix("install-")
        .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
}

pub(crate) fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[cfg(debug_assertions)]
fn development_tool(variable: &str, name: &str, bundled_directory: &Path) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| {
            let bundled = bundled_directory.join(executable_name(name));
            bundled.is_file().then_some(bundled)
        })
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(debug_assertions)]
fn development_ffmpeg(bundled_directory: &Path) -> PathBuf {
    std::env::var_os("BITIG_FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join(executable_name("ffmpeg")))
        .unwrap_or_else(|| development_tool("BITIG_FFMPEG_PATH", "ffmpeg", bundled_directory))
}

#[cfg(test)]
mod tests {
    use super::{
        BundledToolArtifact, BundledToolManifest, CachedToolManifest, InstalledToolVersions,
        ToolPaths, bundled_manifest, bundled_versions, check_cache_layout, comparable_version,
        discover_cached, executable_name, is_installation_name, is_link, is_strictly_newer,
        load_cached, load_cached_installations, plan_tool_cleanup, select_cached, sha256_file,
        valid_hash, write_cached_manifest,
    };

    async fn cached_fixture(
        root: &std::path::Path,
        order: u32,
        yt_dlp: &str,
        ffmpeg: &str,
    ) -> ToolPaths {
        let directory = root.join(format!("install-01900000-0000-7000-8000-{order:012}"));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let paths = ToolPaths {
            yt_dlp: directory.join(executable_name("yt-dlp")),
            ffmpeg: directory.join(executable_name("ffmpeg")),
            ffprobe: directory.join(executable_name("ffprobe")),
            versions: InstalledToolVersions {
                yt_dlp: yt_dlp.into(),
                ffmpeg: ffmpeg.into(),
            },
        };
        for (name, path) in [
            ("yt-dlp", &paths.yt_dlp),
            ("ffmpeg", &paths.ffmpeg),
            ("ffprobe", &paths.ffprobe),
        ] {
            tokio::fs::write(path, format!("{name}:{yt_dlp}:{ffmpeg}"))
                .await
                .unwrap();
        }
        write_cached_manifest(
            &directory,
            &CachedToolManifest {
                schema_version: 1,
                yt_dlp_version: yt_dlp.into(),
                yt_dlp_sha256: sha256_file(&paths.yt_dlp).await.unwrap(),
                ffmpeg_version: ffmpeg.into(),
                ffmpeg_sha256: sha256_file(&paths.ffmpeg).await.unwrap(),
                ffprobe_sha256: sha256_file(&paths.ffprobe).await.unwrap(),
            },
        )
        .await
        .unwrap();
        paths
    }

    async fn bundled_fixture(
        root: &std::path::Path,
        yt_dlp: &str,
        ffmpeg: &str,
    ) -> (ToolPaths, BundledToolManifest) {
        let paths = cached_fixture(&root.join("bundled"), 0, yt_dlp, ffmpeg).await;
        let mut artifacts = Vec::new();
        for (name, path, version) in [
            ("yt-dlp", &paths.yt_dlp, yt_dlp),
            ("ffmpeg", &paths.ffmpeg, ffmpeg),
            ("ffprobe", &paths.ffprobe, ffmpeg),
        ] {
            artifacts.push(BundledToolArtifact {
                name: name.into(),
                version: version.into(),
                sha256: sha256_file(path).await.unwrap(),
            });
        }
        (paths, BundledToolManifest { artifacts })
    }

    #[tokio::test]
    async fn unsafe_yt_dlp_cache_allows_startup_but_blocks_analysis() {
        use crate::downloader::{MediaExtractor, YtDlpExtractor};
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let old = cached_fixture(&root, 1, "2026.10.25", "9.0.1").await;
        let (selected, inventory) = ToolPaths::discover_with_inventory(&root).await.unwrap();
        assert_eq!(selected.yt_dlp, old.yt_dlp);
        assert_eq!(inventory.len(), 1);
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let app_paths = crate::storage::AppPaths {
            root: std::sync::Arc::new(directory.path().to_path_buf()),
            database: directory.path().join("library.db"),
            media: directory.path().join("media"),
            thumbnails: directory.path().join("thumbnails"),
            temp: directory.path().join("temp"),
            tools: root,
            logs: directory.path().join("logs"),
        };
        let library = crate::services::LibraryService::new(
            std::sync::Arc::new(crate::repositories::MediaRepository::new(pool)),
            app_paths,
        );
        assert!(library.list(None).await.unwrap().is_empty());
        let status = crate::services::ToolStatusService::new(selected.versions.clone());
        assert_eq!(status.report(false).await.unwrap().tools.len(), 2);
        let error = YtDlpExtractor::new(selected.yt_dlp)
            .analyze("https://example.invalid/media")
            .await
            .unwrap_err();
        assert!(error.public_message().contains("2026.10.26"), "{error:?}");
        assert_eq!(
            serde_json::to_value(&error).unwrap()["code"],
            "yt_dlp_unsafe"
        );
        assert!(old.yt_dlp.exists());
        assert!(old.ffmpeg.exists());
        assert!(old.ffprobe.exists());
    }

    #[tokio::test]
    async fn safe_cached_yt_dlp_is_reused_without_discarding_newer_ffmpeg() {
        for invalid in ["nightly", "9999.1", "2026.99.99"] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("tools");
            let safe = cached_fixture(&root, 1, "2026.10.26", "9.0.1").await;
            let other = cached_fixture(&root, 2, invalid, "10.0").await;
            let selected = ToolPaths::discover_managed(&root).await.unwrap().unwrap();
            assert_eq!(selected.yt_dlp, safe.yt_dlp, "{invalid}");
            assert_eq!(selected.ffmpeg, other.ffmpeg);
            assert_eq!(selected.ffprobe, other.ffprobe);
            assert!(other.yt_dlp.exists());
        }
    }

    #[tokio::test]
    async fn yt_dlp_processes_require_the_actual_managed_file_and_a_safe_version() {
        use crate::downloader::{
            BoundedProcessRunner, CancellationToken, MediaExtractor, ProcessSpec, YtDlpExtractor,
        };
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("fixture.rs");
        let binary = directory.path().join(executable_name("fixture"));
        std::fs::write(
            &source,
            r##"
fn main() {
    std::fs::write(std::env::current_exe().unwrap().with_extension("started"), b"started").unwrap();
    println!("{}", r#"{"id":"fixture","title":"Fixture"}"#);
}
"##,
        )
        .unwrap();
        let output = std::process::Command::new("rustc")
            .arg("--crate-name=bitig_tool_fixture")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let root = directory.path().join("tools");
        for (order, (version, allowed)) in [
            ("2026.10.25", false),
            ("2026.10.26", true),
            ("2026.11.01", true),
            ("nightly", false),
            ("", false),
            ("2026.13.01", false),
            ("2026.10.26-nightly", false),
            ("2026.10.26 essentials build", false),
        ]
        .into_iter()
        .enumerate()
        {
            let paths = cached_fixture(&root, order as u32, version, "9.0.1").await;
            tokio::fs::copy(&binary, &paths.yt_dlp).await.unwrap();
            let manifest_path = paths.yt_dlp.parent().unwrap().join("manifest.json");
            let mut manifest: CachedToolManifest =
                serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
            manifest.yt_dlp_sha256 = sha256_file(&paths.yt_dlp).await.unwrap();
            write_cached_manifest(paths.yt_dlp.parent().unwrap(), &manifest)
                .await
                .unwrap();
            let marker = paths.yt_dlp.with_extension("started");
            for streaming in [false, true] {
                let result = if streaming {
                    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
                    BoundedProcessRunner
                        .run_cancellable_streaming(
                            ProcessSpec {
                                program: paths.yt_dlp.clone(),
                                is_yt_dlp: true,
                                args: Vec::new(),
                                timeout: std::time::Duration::from_secs(10),
                                stdout_limit: 1024,
                                stderr_limit: 1024,
                            },
                            CancellationToken::default(),
                            sender,
                        )
                        .await
                        .map(|_| ())
                } else {
                    YtDlpExtractor::new(paths.yt_dlp.clone())
                        .analyze("https://example.invalid/media")
                        .await
                        .map(|_| ())
                };
                assert_eq!(
                    result.is_ok(),
                    allowed,
                    "{version}, streaming={streaming}: {result:?}"
                );
                assert_eq!(marker.exists(), allowed, "{version}, streaming={streaming}");
                if allowed {
                    std::fs::remove_file(&marker).unwrap();
                }
            }
        }
        let paths = cached_fixture(&root, 20, "2026.10.26", "9.0.1").await;
        tokio::fs::copy(&binary, &paths.yt_dlp).await.unwrap();
        // The safe version record must not authorize different executable bytes.
        assert!(matches!(
            YtDlpExtractor::new(paths.yt_dlp.clone())
                .analyze("https://example.invalid/media")
                .await,
            Err(crate::errors::AppError::UnsafeYtDlp)
        ));
        assert!(!paths.yt_dlp.with_extension("started").exists());
        let unmanaged = directory.path().join(executable_name("yt-dlp"));
        std::fs::copy(&binary, &unmanaged).unwrap();
        assert!(matches!(
            YtDlpExtractor::new(unmanaged.clone())
                .analyze("https://example.invalid/media")
                .await,
            Err(crate::errors::AppError::UnsafeYtDlp)
        ));
        assert!(!unmanaged.with_extension("started").exists());
        let probe = directory.path().join(executable_name("ffprobe"));
        std::fs::copy(&binary, &probe).unwrap();
        assert!(
            BoundedProcessRunner
                .run(ProcessSpec {
                    program: probe.clone(),
                    is_yt_dlp: false,
                    args: Vec::new(),
                    timeout: std::time::Duration::from_secs(10),
                    stdout_limit: 1024,
                    stderr_limit: 1024,
                })
                .await
                .unwrap()
                .success
        );
        assert!(probe.with_extension("started").exists());
    }

    #[tokio::test]
    async fn retention_plan_handles_empty_small_and_multiple_caches() {
        for count in [0, 1, 2, 5] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("tools");
            let (active, _) = bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
            let mut installations = Vec::new();
            for order in 1..=count {
                installations.push(
                    cached_fixture(
                        &root,
                        order,
                        &format!("2026.09.{order:02}"),
                        &format!("10.{order}"),
                    )
                    .await,
                );
            }
            let inventory = load_cached_installations(&root).await;
            let before = select_cached(&inventory);
            let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
            assert_eq!(plan.retained.len(), (count as usize).min(2));
            assert_eq!(plan.deferred.len(), count.saturating_sub(2) as usize);
            for (index, paths) in installations.iter().enumerate() {
                let path = paths.yt_dlp.parent().unwrap().to_path_buf();
                assert_eq!(plan.retained.contains(&path), index + 2 >= count as usize);
                assert!(load_cached(&path).await.is_ok());
            }
            let after = discover_cached(&root).await;
            assert_eq!(
                before.as_ref().map(|paths| &paths.versions),
                after.as_ref().map(|paths| &paths.versions)
            );
        }
    }

    #[tokio::test]
    async fn retention_keeps_both_group_fallbacks_and_all_active_installations() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let mut installations = Vec::new();
        for (order, yt, ff) in [
            (1, "2026.10.01", "8.0"),
            (2, "2026.09.01", "8.1"),
            (3, "2026.01.01", "11.0"),
            (4, "2025.01.01", "10.0"),
            (5, "2024.01.01", "7.0"),
            (6, "2023.01.01", "6.0"),
            (7, "2022.01.01", "5.0"),
        ] {
            installations.push(cached_fixture(&root, order, yt, ff).await);
        }
        let inventory = load_cached_installations(&root).await;
        let mut active = select_cached(&inventory).unwrap();
        assert_eq!(active.yt_dlp, installations[0].yt_dlp);
        assert_eq!(active.ffmpeg, installations[2].ffmpeg);
        assert_eq!(active.ffprobe, installations[2].ffprobe);
        active.yt_dlp = installations[4].yt_dlp.clone();
        active.ffmpeg = installations[5].ffmpeg.clone();
        active.ffprobe = installations[5].ffprobe.clone();
        let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
        for paths in &installations[..6] {
            assert!(
                plan.retained
                    .contains(&paths.yt_dlp.parent().unwrap().to_path_buf())
            );
        }
        assert_eq!(plan.deferred, [installations[6].yt_dlp.parent().unwrap()]);
        assert!(installations[6].yt_dlp.exists());
    }

    #[tokio::test]
    async fn retention_uses_versions_then_the_existing_uuid_tie_break() {
        for versions in [
            [
                ("2026.10.01", "11.0"),
                ("2026.08.01", "9.0"),
                ("2026.09.01", "10.0"),
            ],
            [
                ("nightly", "N-abc"),
                ("nightly", "N-abc"),
                ("nightly", "N-abc"),
            ],
            [
                ("2026.09.01", "10.0"),
                ("2026.09.01.0", "10.0.0"),
                ("2026.09.01", "10.0"),
            ],
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("tools");
            let (active, _) = bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
            let mut installations = Vec::new();
            for (index, (yt, ff)) in versions.iter().enumerate() {
                installations.push(cached_fixture(&root, index as u32 + 1, yt, ff).await);
            }
            let inventory = load_cached_installations(&root).await;
            let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
            let deferred_index = if versions[0].1 == "11.0" { 1 } else { 0 };
            assert_eq!(
                plan.deferred,
                [installations[deferred_index].yt_dlp.parent().unwrap()]
            );
        }
    }

    #[tokio::test]
    async fn invalid_or_partially_missing_installations_are_not_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let valid = cached_fixture(&root, 1, "2026.09.01", "10.0").await;
        let invalid = cached_fixture(&root, 2, "2026.10.01", "11.0").await;
        std::fs::write(
            invalid.yt_dlp.parent().unwrap().join("manifest.json"),
            b"{broken",
        )
        .unwrap();
        let corrupt = cached_fixture(&root, 3, "2026.11.01", "12.0").await;
        std::fs::write(&corrupt.ffprobe, b"changed").unwrap();
        let partial = cached_fixture(&root, 4, "2026.12.01", "13.0").await;
        std::fs::remove_file(&partial.ffmpeg).unwrap();
        std::fs::create_dir(root.join("install-invalid-uuid")).unwrap();
        std::fs::create_dir(root.join("unrelated")).unwrap();
        std::fs::write(root.join("notes.txt"), b"keep").unwrap();
        let inventory = load_cached_installations(&root).await;
        assert_eq!(inventory.len(), 1);
        let plan = plan_tool_cleanup(&root, &valid, &inventory).unwrap();
        assert!(plan.deferred.is_empty());
        assert_eq!(
            discover_cached(&root).await.unwrap().versions,
            valid.versions
        );
        assert!(invalid.yt_dlp.exists());
        assert_eq!(std::fs::read(&corrupt.ffprobe).unwrap(), b"changed");
        assert!(partial.ffprobe.exists());
        assert_eq!(std::fs::read(root.join("notes.txt")).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn ownership_checks_preserve_extra_content_and_non_v7_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let (active, _) = bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
        let old = cached_fixture(&root, 1, "2026.07.01", "8.0").await;
        cached_fixture(&root, 2, "2026.08.01", "9.0").await;
        cached_fixture(&root, 3, "2026.09.01", "10.0").await;
        let inventory = load_cached_installations(&root).await;
        let path = old.yt_dlp.parent().unwrap();
        for extra_directory in [false, true] {
            let extra = path.join("unexpected");
            if extra_directory {
                std::fs::create_dir(&extra).unwrap();
            } else {
                std::fs::write(&extra, b"keep").unwrap();
            }
            let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
            assert!(plan.deferred.is_empty());
            assert!(plan.retained.contains(&path.to_path_buf()));
            if extra_directory {
                std::fs::remove_dir(&extra).unwrap();
            } else {
                std::fs::remove_file(&extra).unwrap();
            }
        }
        let non_v7 = root.join("install-01900000-0000-4000-8000-000000000001");
        std::fs::rename(path, &non_v7).unwrap();
        assert!(check_cache_layout(&root, &non_v7).is_err());
        let inventory = load_cached_installations(&root).await;
        assert!(
            plan_tool_cleanup(&root, &active, &inventory)
                .unwrap()
                .deferred
                .is_empty()
        );
        assert!(plan_tool_cleanup(directory.path(), &active, &inventory).is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_junctions_are_reparse_points_and_are_preserved() {
        use std::os::windows::{fs::MetadataExt, process::CommandExt};
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let (active, _) = bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
        let old = cached_fixture(&root, 1, "2026.07.01", "8.0").await;
        cached_fixture(&root, 2, "2026.08.01", "9.0").await;
        cached_fixture(&root, 3, "2026.09.01", "10.0").await;
        let inventory = load_cached_installations(&root).await;
        let path = old.yt_dlp.parent().unwrap();
        let target = directory.path().join("junction-target");
        std::fs::rename(path, &target).unwrap();
        let output = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(path)
            .arg(&target)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert_ne!(metadata.file_attributes() & 0x400, 0);
        assert!(is_link(&metadata));
        let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
        assert!(plan.deferred.is_empty());
        assert!(target.join(executable_name("yt-dlp")).exists());
        std::fs::remove_dir(path).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_denied_manifest_reads_preserve_the_installation() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let paths = cached_fixture(&root, 1, "2026.09.01", "10.0").await;
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(paths.yt_dlp.parent().unwrap().join("manifest.json"))
            .unwrap();
        assert!(load_cached_installations(&root).await.is_empty());
        assert!(paths.yt_dlp.exists());
        drop(guard);
        assert!(load_cached(paths.yt_dlp.parent().unwrap()).await.is_ok());
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "Windows symbolic-link creation requires administrator or Developer Mode permission"]
    async fn windows_symbolic_link_files_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        let old = cached_fixture(&root, 1, "2026.07.01", "8.0").await;
        cached_fixture(&root, 2, "2026.08.01", "9.0").await;
        let active = cached_fixture(&root, 3, "2026.09.01", "10.0").await;
        let inventory = load_cached_installations(&root).await;
        let target = directory.path().join("linked-file");
        std::fs::rename(&old.yt_dlp, &target).unwrap();
        std::os::windows::fs::symlink_file(&target, &old.yt_dlp).unwrap();
        assert!(is_link(&std::fs::symlink_metadata(&old.yt_dlp).unwrap()));
        let plan = plan_tool_cleanup(&root, &active, &inventory).unwrap();
        assert!(plan.deferred.is_empty());
        assert!(target.exists());
        std::fs::remove_file(&old.yt_dlp).unwrap();
    }

    #[tokio::test]
    async fn bundled_and_cached_groups_are_selected_by_version() {
        for (cached_versions, use_cached_yt_dlp, use_cached_ffmpeg) in [
            (None, false, false),
            (Some(("2026.07.01", "8.1")), false, false),
            (Some(("2026.09.10", "10.0")), true, true),
            (Some(("2026.08.19", "9.0.1")), false, false),
            (Some(("2026.09.10", "8.1")), true, false),
            (Some(("2026.07.01", "10.0")), false, true),
            (Some(("2026.08.19.123456", "9.0.10")), true, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (bundled, manifest) =
                bundled_fixture(directory.path(), "2026.08.19", "9.0.1 essentials build").await;
            let cache_root = directory.path().join("tools");
            if let Some((yt_dlp, ffmpeg)) = cached_versions {
                cached_fixture(&cache_root, 1, yt_dlp, ffmpeg).await;
            }
            let cached = discover_cached(&cache_root).await;
            let expected_yt_dlp = if use_cached_yt_dlp {
                cached.as_ref().unwrap()
            } else {
                &bundled
            };
            let expected_ffmpeg = if use_cached_ffmpeg {
                cached.as_ref().unwrap()
            } else {
                &bundled
            };
            let selected = bundled
                .clone()
                .select_and_verify(cached.clone(), &manifest)
                .await
                .unwrap();
            assert_eq!(
                selected.yt_dlp, expected_yt_dlp.yt_dlp,
                "{cached_versions:?}"
            );
            assert_eq!(
                selected.ffmpeg, expected_ffmpeg.ffmpeg,
                "{cached_versions:?}"
            );
            assert_eq!(
                selected.ffprobe, expected_ffmpeg.ffprobe,
                "{cached_versions:?}"
            );
            assert_eq!(selected.ffmpeg.parent(), selected.ffprobe.parent());
            assert_eq!(selected.versions.yt_dlp, expected_yt_dlp.versions.yt_dlp);
            assert_eq!(selected.versions.ffmpeg, expected_ffmpeg.versions.ffmpeg);
        }
    }

    #[tokio::test]
    async fn cached_versions_take_precedence_over_installation_order() {
        let directory = tempfile::tempdir().unwrap();
        let newer = cached_fixture(directory.path(), 1, "2026.09.10", "10.0").await;
        cached_fixture(directory.path(), 2, "2026.07.01", "8.1").await;
        let selected = discover_cached(directory.path()).await.unwrap();
        assert_eq!(selected.yt_dlp, newer.yt_dlp);
        assert_eq!(selected.ffmpeg, newer.ffmpeg);
        assert_eq!(selected.ffprobe, newer.ffprobe);
        assert_eq!(selected.versions, newer.versions);
    }

    #[test]
    fn numeric_versions_require_complete_and_non_overflowing_components() {
        for (newer, older) in [
            ("2026.08.19.123456", "2026.08.19"),
            ("9.0.10", "9.0.9"),
            ("10.0", "9.0.1 essentials build"),
            ("v10.0", "9.0.1"),
        ] {
            assert!(is_strictly_newer(newer, older), "{newer} > {older}");
            assert!(!is_strictly_newer(older, newer));
            assert!(!is_strictly_newer(newer, newer));
        }
        assert!(!is_strictly_newer("9.0.1.0", "9.0.1 essentials build"));
        assert!(!is_strictly_newer("9.0.1", "9.0.1.0"));
        for invalid in [
            "",
            " ",
            ".9",
            "9.",
            "9..1",
            "9.+1",
            "9.-1",
            "9/1",
            "9.0beta",
            "18446744073709551616.0",
            "9.18446744073709551616",
            "nightly",
            "2026.08.19-nightly",
            "N-123456-gabcdef",
            "10.0-dev",
            "10.0-rc1",
        ] {
            assert_eq!(comparable_version(invalid), None, "{invalid}");
            assert!(!is_strictly_newer(invalid, "9.0.1"));
            assert!(!is_strictly_newer("10.0", invalid));
        }
    }

    #[tokio::test]
    async fn incomparable_versions_prefer_verified_bundle() {
        for (bundled_yt, bundled_ff, cached_yt, cached_ff) in [
            ("2026.08.19", "9.0.1", "nightly", "N-123456-gabcdef"),
            ("2026.08.19", "9.0.1", "2026.09.01-nightly", "10.0-rc1"),
            (
                "2026.08.19",
                "9.0.1",
                "18446744073709551616",
                "18446744073709551616",
            ),
            ("nightly", "N-123456-gabcdef", "2026.09.10", "10.0"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (bundled, manifest) =
                bundled_fixture(directory.path(), bundled_yt, bundled_ff).await;
            let cache_root = directory.path().join("tools");
            cached_fixture(&cache_root, 1, cached_yt, cached_ff).await;
            let selected = bundled
                .clone()
                .select_and_verify(discover_cached(&cache_root).await, &manifest)
                .await
                .unwrap();
            assert_eq!(selected.yt_dlp, bundled.yt_dlp);
            assert_eq!(selected.ffmpeg, bundled.ffmpeg);
            assert_eq!(selected.ffprobe, bundled.ffprobe);
            assert_eq!(selected.versions, bundled.versions);
        }
    }

    #[tokio::test]
    async fn cached_groups_can_come_from_different_verified_installations() {
        let directory = tempfile::tempdir().unwrap();
        let yt_dlp = cached_fixture(directory.path(), 1, "2026.09.10", "8.1").await;
        let ffmpeg = cached_fixture(directory.path(), 2, "2026.07.01", "10.0").await;
        cached_fixture(directory.path(), 3, "nightly", "N-123456-gabcdef").await;
        let selected = discover_cached(directory.path()).await.unwrap();
        assert_eq!(selected.yt_dlp, yt_dlp.yt_dlp);
        assert_eq!(selected.ffmpeg, ffmpeg.ffmpeg);
        assert_eq!(selected.ffprobe, ffmpeg.ffprobe);
        assert_eq!(selected.versions.yt_dlp, yt_dlp.versions.yt_dlp);
        assert_eq!(selected.versions.ffmpeg, ffmpeg.versions.ffmpeg);
    }

    #[tokio::test]
    async fn cached_ties_and_unordered_fallback_keep_installation_order() {
        for (yt_dlp, ffmpeg) in [("2026.09.10", "10.0"), ("nightly", "N-123456-gabcdef")] {
            let directory = tempfile::tempdir().unwrap();
            cached_fixture(directory.path(), 1, yt_dlp, ffmpeg).await;
            let latest = cached_fixture(directory.path(), 2, yt_dlp, ffmpeg).await;
            let selected = discover_cached(directory.path()).await.unwrap();
            assert_eq!(selected.yt_dlp, latest.yt_dlp);
            assert_eq!(selected.ffmpeg, latest.ffmpeg);
            assert_eq!(selected.ffprobe, latest.ffprobe);
        }
    }

    #[tokio::test]
    async fn corrupt_cached_files_reject_the_entire_installation() {
        for tool in ["yt-dlp", "ffmpeg", "ffprobe"] {
            let directory = tempfile::tempdir().unwrap();
            let corrupted = cached_fixture(directory.path(), 2, "2026.09.10", "10.0").await;
            let installation = corrupted.yt_dlp.parent().unwrap();
            tokio::fs::write(installation.join(executable_name(tool)), b"tampered")
                .await
                .unwrap();
            assert!(load_cached(installation).await.is_err());
            assert!(discover_cached(directory.path()).await.is_none());
            let valid = cached_fixture(directory.path(), 1, "2026.07.01", "8.1").await;
            let selected = discover_cached(directory.path()).await.unwrap();
            assert_eq!(selected.yt_dlp, valid.yt_dlp);
            assert_eq!(selected.ffmpeg, valid.ffmpeg);
            assert_eq!(selected.ffprobe, valid.ffprobe);
            assert_eq!(selected.versions, valid.versions);
        }
    }

    #[tokio::test]
    async fn invalid_cached_version_metadata_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        for invalid in ["", " ", "9/1", &"9".repeat(65)] {
            for (yt_dlp, ffmpeg) in [(invalid, "10.0"), ("2026.09.10", invalid)] {
                let paths = cached_fixture(directory.path(), 1, yt_dlp, ffmpeg).await;
                assert!(load_cached(paths.yt_dlp.parent().unwrap()).await.is_err());
                assert!(discover_cached(directory.path()).await.is_none());
            }
        }
    }

    #[tokio::test]
    async fn bundled_hash_failures_fall_back_only_for_the_affected_group() {
        for tool in ["yt-dlp", "ffmpeg", "ffprobe"] {
            let directory = tempfile::tempdir().unwrap();
            let (bundled, manifest) =
                bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
            let installation = bundled.yt_dlp.parent().unwrap();
            tokio::fs::write(installation.join(executable_name(tool)), b"tampered")
                .await
                .unwrap();
            let error = bundled
                .clone()
                .select_and_verify(None, &manifest)
                .await
                .unwrap_err();
            assert!(matches!(error, crate::errors::AppError::ToolIntegrity(name) if name == tool));
            let cache_root = directory.path().join("tools");
            cached_fixture(&cache_root, 1, "2026.07.01", "8.1").await;
            let cached = discover_cached(&cache_root).await.unwrap();
            let selected = bundled
                .clone()
                .select_and_verify(Some(cached.clone()), &manifest)
                .await
                .unwrap();
            let expected_yt_dlp = if tool == "yt-dlp" { &cached } else { &bundled };
            let expected_ffmpeg = if tool == "yt-dlp" { &bundled } else { &cached };
            assert_eq!(selected.yt_dlp, expected_yt_dlp.yt_dlp);
            assert_eq!(selected.ffmpeg, expected_ffmpeg.ffmpeg);
            assert_eq!(selected.ffprobe, expected_ffmpeg.ffprobe);
            assert_eq!(selected.versions.yt_dlp, expected_yt_dlp.versions.yt_dlp);
            assert_eq!(selected.versions.ffmpeg, expected_ffmpeg.versions.ffmpeg);
        }
    }

    #[tokio::test]
    async fn missing_bundled_files_or_hashes_require_a_verified_fallback() {
        for tool in ["yt-dlp", "ffmpeg", "ffprobe"] {
            for missing_file in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let (bundled, mut manifest) =
                    bundled_fixture(directory.path(), "2026.08.19", "9.0.1").await;
                if missing_file {
                    tokio::fs::remove_file(
                        bundled.yt_dlp.parent().unwrap().join(executable_name(tool)),
                    )
                    .await
                    .unwrap();
                } else {
                    manifest.artifacts.retain(|artifact| artifact.name != tool);
                }
                assert!(
                    bundled
                        .clone()
                        .select_and_verify(None, &manifest)
                        .await
                        .is_err()
                );
                let cache_root = directory.path().join("tools");
                cached_fixture(&cache_root, 1, "nightly", "N-123456-gabcdef").await;
                let selected = bundled
                    .select_and_verify(discover_cached(&cache_root).await, &manifest)
                    .await
                    .unwrap();
                assert_eq!(selected.ffmpeg.parent(), selected.ffprobe.parent());
                let path = if tool == "yt-dlp" {
                    &selected.yt_dlp
                } else {
                    &selected.ffmpeg
                };
                assert!(path.starts_with(&cache_root));
            }
        }
    }

    #[tokio::test]
    async fn discovery_preserves_debug_and_release_boundaries() {
        const CASE: &str = "BITIG_T31_DISCOVERY_CASE";
        const ROOT: &str = "BITIG_T31_DISCOVERY_ROOT";
        if let Ok(case) = std::env::var(CASE) {
            let root = std::path::PathBuf::from(std::env::var_os(ROOT).unwrap());
            #[cfg(debug_assertions)]
            if case == "path" {
                let empty = root.join("empty");
                assert_eq!(
                    super::development_tool("BITIG_YTDLP_PATH", "yt-dlp", &empty),
                    std::path::PathBuf::from("yt-dlp")
                );
                assert_eq!(
                    super::development_tool("BITIG_FFPROBE_PATH", "ffprobe", &empty),
                    std::path::PathBuf::from("ffprobe")
                );
                assert_eq!(
                    super::development_ffmpeg(&empty),
                    std::path::PathBuf::from("ffmpeg")
                );
                let bundled = cached_fixture(&root, 0, "2026.08.19", "9.0.1").await;
                let directory = bundled.yt_dlp.parent().unwrap();
                assert_eq!(
                    super::development_tool("BITIG_YTDLP_PATH", "yt-dlp", directory),
                    bundled.yt_dlp
                );
                assert_eq!(
                    super::development_tool("BITIG_FFPROBE_PATH", "ffprobe", directory),
                    bundled.ffprobe
                );
                assert_eq!(super::development_ffmpeg(directory), bundled.ffmpeg);
                assert!(bundled.are_bundled_in(directory));
                let mut external = bundled.clone();
                external.ffprobe = root.join("external-probe");
                assert!(!external.are_bundled_in(directory));
                return;
            }
            let result = ToolPaths::discover_and_verify(&root.join("tools")).await;
            if case == "cache" {
                let expected = discover_cached(&root.join("tools")).await.unwrap();
                let selected = result.unwrap();
                assert_eq!(selected.yt_dlp, expected.yt_dlp);
                assert_eq!(selected.ffmpeg, expected.ffmpeg);
                assert_eq!(selected.ffprobe, expected.ffprobe);
                assert_eq!(selected.versions, expected.versions);
            } else {
                #[cfg(debug_assertions)]
                {
                    let selected = result.unwrap();
                    assert_eq!(selected.yt_dlp, root.join("external-yt"));
                    assert_eq!(selected.ffprobe, root.join("external-probe"));
                    assert_eq!(
                        selected.ffmpeg,
                        if case == "directory" {
                            root.join("ffmpeg-directory")
                                .join(executable_name("ffmpeg"))
                        } else {
                            root.join("external-ffmpeg")
                        }
                    );
                }
                #[cfg(not(debug_assertions))]
                assert!(matches!(
                    result,
                    Err(crate::errors::AppError::ToolUnavailable)
                ));
            }
            return;
        }

        // Child environments avoid mutating process-wide variables used by parallel tests.
        for case in ["paths", "directory", "path", "cache"] {
            let directory = tempfile::tempdir().unwrap();
            if case == "cache" {
                cached_fixture(&directory.path().join("tools"), 1, "9999.1", "9999.1").await;
            }
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "downloader::tools::tests::discovery_preserves_debug_and_release_boundaries",
                    "--nocapture",
                ])
                .env(CASE, case)
                .env(ROOT, directory.path());
            for variable in [
                "BITIG_YTDLP_PATH",
                "BITIG_FFPROBE_PATH",
                "BITIG_FFMPEG_PATH",
                "BITIG_FFMPEG_DIR",
            ] {
                command.env_remove(variable);
            }
            if case != "path" {
                command
                    .env("BITIG_YTDLP_PATH", directory.path().join("external-yt"))
                    .env(
                        "BITIG_FFPROBE_PATH",
                        directory.path().join("external-probe"),
                    )
                    .env(
                        "BITIG_FFMPEG_PATH",
                        directory.path().join("external-ffmpeg"),
                    );
            }
            if case == "directory" {
                command.env(
                    "BITIG_FFMPEG_DIR",
                    directory.path().join("ffmpeg-directory"),
                );
            }
            let output = command.output().await.unwrap();
            assert!(
                output.status.success(),
                "{case}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn online_release_requires_verified_managed_tools_and_preserves_legacy_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("tools");
        std::fs::create_dir(&root).unwrap();
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            std::fs::write(root.join(executable_name(name)), b"legacy unmanaged tool").unwrap();
        }
        assert!(matches!(
            ToolPaths::discover_with_inventory(&root).await,
            Err(crate::errors::AppError::ToolUnavailable)
        ));
        let expected = cached_fixture(&root, 1, "2025.01.01", "8.0").await;
        let (selected, inventory) = ToolPaths::discover_with_inventory(&root).await.unwrap();
        assert_eq!(selected.yt_dlp, expected.yt_dlp);
        assert_eq!(selected.ffmpeg, expected.ffmpeg);
        assert_eq!(selected.ffprobe, expected.ffprobe);
        assert_eq!(inventory.len(), 1);
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            assert_eq!(
                std::fs::read(root.join(executable_name(name))).unwrap(),
                b"legacy unmanaged tool"
            );
        }
    }

    #[test]
    fn manifest_has_versions_and_sha256_for_every_required_tool() {
        let manifest = bundled_manifest().unwrap();
        let versions = bundled_versions(&manifest).unwrap();
        assert!(!versions.yt_dlp.is_empty());
        assert!(!versions.ffmpeg.is_empty());
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            let artifact = manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.name == name)
                .unwrap();
            assert!(valid_hash(&artifact.sha256));
        }
    }

    #[test]
    fn cached_installation_names_reject_traversal_and_arbitrary_directories() {
        assert!(is_installation_name(
            "install-018f0c30-7b14-7cc1-8db7-5d2c557f74cf"
        ));
        assert!(!is_installation_name("install-../media"));
        assert!(!is_installation_name("current"));
    }
}
