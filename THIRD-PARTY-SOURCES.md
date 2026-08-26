# Third-party corresponding source

BITIG's Windows x64 release bundles unmodified command-line sidecars. Every
release must use the exact binary SHA-256 and source revisions recorded in
`src-tauri/binaries/manifest.json`. The manifest is the machine-readable source
of truth; this document explains how to obtain the corresponding source.

## yt-dlp.exe

- Binary release: yt-dlp `2026.08.19`
- Binary SHA-256: `66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a`
- Exact upstream tag: `2026.08.19`
- Exact upstream commit: `3a08beaf031ab68f966401ead017ac81fe8486cf`
- Corresponding source archive: <https://github.com/yt-dlp/yt-dlp/archive/3a08beaf031ab68f966401ead017ac81fe8486cf.tar.gz>
- Source archive SHA-256: `7206981142eb461cfa603c360a55e0d08f3ed58cc754000ed821ad3ebac31ea0`
- Binary download: <https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe>
- Binary distribution license: `GPL-3.0-or-later`
- yt-dlp core license: `Unlicense`
- Bundled-component notices: `licenses/yt-dlp-THIRD-PARTY-LICENSES.txt`

The PyInstaller executable includes third-party components. Its bundled notice
identifies GPL-2.0-or-later components, so BITIG records and distributes the
aggregate executable under GPL-3.0-or-later while preserving the distinct
Unlicense status of the yt-dlp core. The complete upstream notices and core
license are included in every BITIG installer.

## ffmpeg.exe and ffprobe.exe

- Binary release: Gyan FFmpeg `9.0.1 essentials build`
- ffmpeg.exe SHA-256: `72a489eccd008c2ec2c0a5856c5c75bc3d8bbfa90166c4566865c246445e6aa3`
- ffprobe.exe SHA-256: `19202b23c0043f15ad1b7bce2344f406fd52bd6efd8f995ce02e7392a1cec52f`
- Build-provider tag and commit: `9.0.1` / `46465995c991fe65c5de853fa79bddec09cd6c37`
- Exact FFmpeg tag and commit: `n9.0.1` / `bf1b838f2ab88b4f8fd83443325c782ea0e0f7fa`
- Corresponding source archive: <https://github.com/FFmpeg/FFmpeg/archive/bf1b838f2ab88b4f8fd83443325c782ea0e0f7fa.tar.gz>
- Source archive SHA-256: `fb1931fd4eb29297ee1c1017a24f800c4d8fbea35b4f2aaeb28308a48a9149b4`
- Binary archive: <https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.1-essentials_build.zip>
- License: `GPL-3.0-or-later`
- Complete build configuration and provenance: `licenses/FFmpeg-SOURCE-AND-BUILD-INFO.txt`
- Exact upstream licensing and external-library record: `licenses/FFmpeg-UPSTREAM-LICENSE.md`
- License text: `licenses/FFmpeg-GPLv3.txt`

ffmpeg and ffprobe come from the same archive and source revision. BITIG does
not patch either executable.

## Release requirement

The release workflow downloads the pinned binaries, verifies their individual
SHA-256 values, builds BITIG only after verification succeeds, and publishes a
SHA-256 checksum file for release artifacts. A release maintainer must also
attach or otherwise keep reachable the exact source archives linked above. If
an upstream source becomes unavailable, that release must not be rebuilt or
redistributed until an exact mirror is available and documented here.
