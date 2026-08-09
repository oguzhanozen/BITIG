# Third-party corresponding source

BITIG's Windows x64 release bundles unmodified command-line sidecars. Every
release must use the exact binary SHA-256 and source revisions recorded in
`src-tauri/binaries/manifest.json`. The manifest is the machine-readable source
of truth; this document explains how to obtain the corresponding source.

## yt-dlp.exe

- Binary release: yt-dlp `2026.07.04`
- Binary SHA-256: `52fe3c26dcf71fbdc85b528589020bb0b8e383155cfa81b64dd447bbe35e24b8`
- Exact upstream tag: `2026.07.04`
- Exact upstream commit: `fdec00e0bf530dc6c3cc7b1dd780e95d9ae460e9`
- Corresponding source archive: <https://github.com/yt-dlp/yt-dlp/archive/fdec00e0bf530dc6c3cc7b1dd780e95d9ae460e9.tar.gz>
- Source archive SHA-256: `27c51a76a68621313f678aa8b9c48ce39b895e0db79e06963e07c1b1662c4786`
- Binary download: <https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp.exe>
- Binary distribution license: `GPL-3.0-or-later`
- yt-dlp core license: `Unlicense`
- Bundled-component notices: `licenses/yt-dlp-THIRD-PARTY-LICENSES.txt`

The PyInstaller executable includes third-party components. Its bundled notice
identifies GPL-2.0-or-later components, so BITIG records and distributes the
aggregate executable under GPL-3.0-or-later while preserving the distinct
Unlicense status of the yt-dlp core. The complete upstream notices and core
license are included in every BITIG installer.

## ffmpeg.exe and ffprobe.exe

- Binary release: Gyan FFmpeg `9.0 essentials build`
- ffmpeg.exe SHA-256: `227af0691433b703ffc5725e47f7d06eefc34b4a72e7870e73d30e2cda483ecf`
- ffprobe.exe SHA-256: `901f0efe4793cbb0f017101e3427f816e8fbf9a407bd585f49df30f4325cfd88`
- Build-provider tag and commit: `9.0` / `46465995c991fe65c5de853fa79bddec09cd6c37`
- Exact FFmpeg tag and commit: `n9.0` / `d32b387f2b0a484599d4587d651891f0c63c4238`
- Corresponding source archive: <https://github.com/FFmpeg/FFmpeg/archive/d32b387f2b0a484599d4587d651891f0c63c4238.tar.gz>
- Source archive SHA-256: `8a830a34bfaf98514b5d45cf6c01b1fe78b38d5e4c10eab0de2531b783c15f90`
- Binary archive: <https://github.com/GyanD/codexffmpeg/releases/download/9.0/ffmpeg-9.0-essentials_build.zip>
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
