use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::errors::{AppError, AppResult};

const MANIFEST: &str = include_str!("../../binaries/manifest.json");

#[derive(Clone, Debug)]
pub struct ToolPaths {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Deserialize)]
struct ToolManifest {
    artifacts: Vec<ToolArtifact>,
}

#[derive(Deserialize)]
struct ToolArtifact {
    name: String,
    sha256: String,
}

impl ToolPaths {
    pub async fn discover_and_verify() -> AppResult<Self> {
        let bundled_directory = std::env::current_exe()
            .map_err(|_| AppError::ToolUnavailable)?
            .parent()
            .map(ToOwned::to_owned)
            .ok_or(AppError::ToolUnavailable)?;

        #[cfg(not(debug_assertions))]
        let paths = Self {
            yt_dlp: bundled_directory.join(executable_name("yt-dlp")),
            ffmpeg: bundled_directory.join(executable_name("ffmpeg")),
            ffprobe: bundled_directory.join(executable_name("ffprobe")),
        };

        #[cfg(debug_assertions)]
        let paths = Self {
            yt_dlp: development_tool("BITIG_YTDLP_PATH", "yt-dlp", &bundled_directory),
            ffmpeg: development_ffmpeg(&bundled_directory),
            ffprobe: development_tool("BITIG_FFPROBE_PATH", "ffprobe", &bundled_directory),
        };

        #[cfg(not(debug_assertions))]
        paths.verify_bundled().await?;

        #[cfg(debug_assertions)]
        if paths.are_bundled_in(&bundled_directory) {
            paths.verify_bundled().await?;
        }

        Ok(paths)
    }

    async fn verify_bundled(&self) -> AppResult<()> {
        let expected = expected_hashes()?;
        for (name, path) in [
            ("yt-dlp", &self.yt_dlp),
            ("ffmpeg", &self.ffmpeg),
            ("ffprobe", &self.ffprobe),
        ] {
            if !path.is_file() {
                return Err(AppError::ToolUnavailable);
            }
            let expected_hash = expected
                .get(name)
                .ok_or_else(|| AppError::ToolIntegrity(name.into()))?;
            let actual_hash = sha256_file(path).await?;
            if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                tracing::error!(tool = name, "bundled media tool hash mismatch");
                return Err(AppError::ToolIntegrity(name.into()));
            }
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn are_bundled_in(&self, directory: &std::path::Path) -> bool {
        [&self.yt_dlp, &self.ffmpeg, &self.ffprobe]
            .iter()
            .all(|path| path.parent() == Some(directory))
    }
}

fn expected_hashes() -> AppResult<HashMap<String, String>> {
    let manifest: ToolManifest = serde_json::from_str(MANIFEST)
        .map_err(|error| AppError::ToolIntegrity(error.to_string()))?;
    Ok(manifest
        .artifacts
        .into_iter()
        .map(|artifact| (artifact.name, artifact.sha256))
        .collect())
}

async fn sha256_file(path: &std::path::Path) -> AppResult<String> {
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

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[cfg(debug_assertions)]
fn development_tool(variable: &str, name: &str, bundled_directory: &std::path::Path) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| {
            let bundled = bundled_directory.join(executable_name(name));
            bundled.is_file().then_some(bundled)
        })
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(debug_assertions)]
fn development_ffmpeg(bundled_directory: &std::path::Path) -> PathBuf {
    std::env::var_os("BITIG_FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join(executable_name("ffmpeg")))
        .unwrap_or_else(|| development_tool("BITIG_FFMPEG_PATH", "ffmpeg", bundled_directory))
}

#[cfg(test)]
mod tests {
    use super::expected_hashes;

    #[test]
    fn manifest_has_a_sha256_for_every_required_tool() {
        let hashes = expected_hashes().unwrap();
        for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
            let hash = hashes.get(name).unwrap();
            assert_eq!(hash.len(), 64);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
}
