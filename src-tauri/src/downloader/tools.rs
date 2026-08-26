use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::errors::{AppError, AppResult};

const BUNDLED_MANIFEST: &str = include_str!("../../binaries/manifest.json");
const CACHED_MANIFEST: &str = "manifest.json";

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

#[derive(Deserialize)]
struct BundledToolManifest {
    artifacts: Vec<BundledToolArtifact>,
}

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
    pub async fn discover_and_verify(updated_root: &Path) -> AppResult<Self> {
        if let Some(cached) = discover_cached(updated_root).await {
            return Ok(cached);
        }

        let bundled_directory = std::env::current_exe()
            .map_err(|_| AppError::ToolUnavailable)?
            .parent()
            .map(ToOwned::to_owned)
            .ok_or(AppError::ToolUnavailable)?;
        let manifest = bundled_manifest()?;

        #[cfg(not(debug_assertions))]
        let paths = Self {
            yt_dlp: bundled_directory.join(executable_name("yt-dlp")),
            ffmpeg: bundled_directory.join(executable_name("ffmpeg")),
            ffprobe: bundled_directory.join(executable_name("ffprobe")),
            versions: bundled_versions(&manifest)?,
        };

        #[cfg(debug_assertions)]
        let paths = Self {
            yt_dlp: development_tool("BITIG_YTDLP_PATH", "yt-dlp", &bundled_directory),
            ffmpeg: development_ffmpeg(&bundled_directory),
            ffprobe: development_tool("BITIG_FFPROBE_PATH", "ffprobe", &bundled_directory),
            versions: bundled_versions(&manifest)?,
        };

        #[cfg(not(debug_assertions))]
        paths.verify_bundled(&manifest).await?;

        #[cfg(debug_assertions)]
        if paths.are_bundled_in(&bundled_directory) {
            paths.verify_bundled(&manifest).await?;
        }

        Ok(paths)
    }

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

async fn discover_cached(updated_root: &Path) -> Option<ToolPaths> {
    let mut entries = match tokio::fs::read_dir(updated_root).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "updated tool directory could not be read");
            return None;
        }
    };
    let mut candidates = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_installation_name(&name) || !entry.file_type().await.is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        candidates.push((name, entry.path()));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    for (_, directory) in candidates {
        match load_cached(&directory).await {
            Ok(paths) => return Some(paths),
            Err(error) => tracing::warn!(%error, "cached media tools were ignored"),
        }
    }
    None
}

async fn load_cached(directory: &Path) -> AppResult<ToolPaths> {
    let manifest_path = directory.join(CACHED_MANIFEST);
    let metadata = tokio::fs::metadata(&manifest_path).await?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(AppError::ToolIntegrity("cached manifest size".into()));
    }
    let manifest: CachedToolManifest =
        serde_json::from_slice(&tokio::fs::read(manifest_path).await?)
            .map_err(|error| AppError::ToolIntegrity(error.to_string()))?;
    validate_cached_manifest(&manifest)?;
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
        if !path.is_file() {
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
    }
    Ok(())
}

fn bundled_manifest() -> AppResult<BundledToolManifest> {
    serde_json::from_str(BUNDLED_MANIFEST)
        .map_err(|error| AppError::ToolIntegrity(error.to_string()))
}

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
    let mut file = tokio::fs::File::open(path).await?;
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
    use super::{bundled_manifest, bundled_versions, is_installation_name, valid_hash};

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
