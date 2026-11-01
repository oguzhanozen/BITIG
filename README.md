# BITIG

**Save it. Sort it. Keep it.**

BITIG is a local-first Windows desktop media downloader and organized media library. Paste a supported media URL, analyze the available video and audio formats, choose what to save, follow the download, then play and organize the result in nested virtual folders.

Built with Tauri 2, Rust, React, TypeScript, and SQLite. Downloads are managed and verified before they enter the library.

**Supported platform:** Windows x64 · [Download BITIG v0.1.5](https://github.com/oguzhanozen/BITIG/releases/tag/v0.1.5)

BITIG v0.1.5 is the latest published version and the final planned application release. BITIG has no application self-updater; media tool updates are managed separately.

## See BITIG in action

![BITIG desktop demo showing a media analysis, format choices, download progress, and the organized library](docs/media/bitig-demo.gif)

The 15-second looping preview follows the app's flow: paste a link → analyze → choose video or audio → start a download → watch its progress → find the saved media in the library and folders.

## Features

- Download video or audio from supported media URLs, with available format choices and download progress.
- Play saved media in the built-in player, with Fullscreen and Theater Mode for video. Playback depends on the available files and supported codecs.
- Browse and search a local media library, including while offline.
- Create, rename, reorder, and delete nested virtual folders; rename media and move it between folders.
- Export media as a file copy through a native save dialog.
- View yt-dlp and FFmpeg/ffprobe status in About & Tools, check for updates, and install them manually or enable optional automatic tool updates.

## Legal / acceptable use

BITIG should only be used for content you have the right to download, copy, or store. Users are responsible for complying with applicable copyright law and terms of service. BITIG is not designed to bypass DRM or access controls.

## Installation and media tools

1. Open the [v0.1.5 release](https://github.com/oguzhanozen/BITIG/releases/tag/v0.1.5) and download `BITIG_0.1.5_x64-setup.exe` from **Assets**.
2. Run the installer, read and accept the displayed terms, and complete setup. Automatic media tool updates are optional and off by default.
3. Launch BITIG, paste a supported media URL, and choose video or audio to download.

End users do not need Node.js, Rust, Python, or a source build. The Windows x64
Online Installer does **not** embed yt-dlp, FFmpeg, or ffprobe in `setup.exe`.
These are required components that cannot be deselected. Setup reuses existing
verified BITIG-managed tools when available; otherwise it downloads yt-dlp and
the GyanD FFmpeg essentials build (including ffprobe) from GitHub over HTTPS
with SHA-256 verification. Separate tools on system `PATH` or legacy packaged
tools are not used by the release application. Setup stops if required tools
cannot be obtained and verified.

WebView2 uses an **embedded bootstrapper**, which downloads and installs the
runtime if it is missing. Internet access is required when media tools or the
WebView2 Runtime need to be downloaded. Setup requires interactive terms
acceptance; silent and passive installation are unsupported.

The installer is unsigned (no Authenticode digital signature), so Windows
SmartScreen may show a warning. The release includes
[SHA256SUMS.txt](https://github.com/oguzhanozen/BITIG/releases/download/v0.1.5/SHA256SUMS.txt)
for checking the downloaded installer's hash.

BITIG requires **yt-dlp 2026.10.26 or newer** as its minimum safe version and
refuses to run an older or unverifiable yt-dlp executable. About & Tools
provides manual tool checks and installation. Automatic tool updates require
your explicit opt-in and can be enabled or disabled there, separately from
installer terms acceptance. An existing saved application choice takes
priority over the installer preference. Restart BITIG to use newly installed
tools; these updates do not update the BITIG application itself.

The [installer terms and notices](licenses/INSTALLER-TERMS.txt) document setup
behavior, licenses, and local data/network connections.

## Development

### Prerequisites

- Node.js 24+ and npm 11+.
- Rust 1.94+ with the Windows platform prerequisites required by Tauri 2.
- For media analysis and downloads, verified BITIG-managed tools provisioned by the Windows installer above. Development reuses these installations; standalone `PATH` tools and the historical pinned yt-dlp reference do not satisfy the current yt-dlp runtime verification.

From a checkout of this repository:

```text
npm ci
npm run tauri dev
```

Frontend-only development is available with `npm run dev`, although Tauri commands require the desktop shell.
See [the sidecar development notes](docs/development/sidecars.md) for path
overrides and reference binary provenance; those references do not replace
the runtime verification requirement above.

## Release build

Run `npm run tauri:build` to build the Windows x64 NSIS Online Installer.
It verifies reference provenance and notices without requiring local media
binaries, then checks the actual package with 7-Zip. Package inspection requires
`7z.exe` on PATH or `BITIG_7ZIP_PATH`. Media executables are excluded from the
installer through an empty `bundle.externalBin`; WebView2 uses
`embedBootstrapper`. The default `npm run sidecars:verify` checks historical
reference binaries separately. See [the sidecar development notes](docs/development/sidecars.md)
and [the release workflow](.github/workflows/release.yml) for package and
publication checks.

## Quality checks

```text
npm run typecheck
npm run lint
npm run test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

## Local data

Tauri resolves the OS-specific AppLocalData directory. BITIG creates `library.db`, `media/`, `thumbnails/`, `temp/`, `tools/`, and `logs/` below that root. Bootstrap stores its update preference in `installer-preferences.json`; UI preferences use WebView2 localStorage. Managed tools use `tools/install-<UUIDv7>/manifest.json` to record versions and binary SHA-256 values. Physical deletion of old tool installations is disabled for process safety, so disk use can grow over time. UI folders are SQLite records; moving or renaming media does not rename the internal media file.

Media URLs and metadata may be stored locally. Sensitive URL parameter filtering
does not guarantee removal of all personal information. Saved library features
work offline; online media operations contact media services, while necessary
tool provisioning and manual or authorized automatic tool updates contact
GitHub. See Part III of the installer document for the informational disclosure.

## Font and licensing

BITIG source code is released under the MIT License; see [LICENSE](LICENSE).

Press Start 2P is bundled under `src/assets/fonts/` so the desktop UI works offline. Its [SIL Open Font License](licenses/PressStart2P-OFL.txt) is preserved in the repository. The font asset and license are sourced from the official Google Fonts repository.

The sidecar manifest and [THIRD-PARTY-SOURCES.md](THIRD-PARTY-SOURCES.md) preserve
historical reference versions, exact source revisions, archive locations,
and hashes. They do not specify the versions delivered by the Online Installer
or later tool updates. The source document explains how to find sources and
notices for the version actually installed.
After any required restart, About & Tools shows the selected yt-dlp and FFmpeg
build versions; ffprobe comes from the same FFmpeg build. Installed versions
can differ from the baseline and the two tool groups can use different managed
installations.

yt-dlp's core is Unlicense; its Windows PyInstaller executable is a GPLv3+
combined distribution. The inspected GyanD FFmpeg/ffprobe build is GPLv3+;
GyanD supplies Windows binaries and is separate from the FFmpeg development
project. These tools run as separate subprocesses. This technical separation
and online downloading do not remove applicable licensing duties.

Complete license texts, yt-dlp notices, FFmpeg source/build information,
Rust dependency notices, Node/React notices, and the font license are preserved
under [licenses/](licenses/). The release resource mapping includes that directory,
maps `LICENSE` to `licenses/BITIG-MIT.txt`, and maps the source index to
`licenses/THIRD-PARTY-SOURCES.md` under the installation directory. Package
verification checks all eleven license/notice/source documents against their
repository hashes. Preserved reference notices do not cover every later tool
version; consult [THIRD-PARTY-SOURCES.md](THIRD-PARTY-SOURCES.md) for version-specific
notices and corresponding source/build information.
The [installer document](licenses/INSTALLER-TERMS.txt) preserves MIT
and third-party open-source rights; it does not replace their license texts.

The reference FFmpeg build includes code derived from the Independent JPEG Group. BITIG uses the GyanD binaries unchanged and makes no additions or deletions to that code. The [preserved upstream licensing record](licenses/FFmpeg-UPSTREAM-LICENSE.md) identifies its source revision in its header; version-specific information must also be checked against the actual build's sources.

## Troubleshooting

- If media tools are missing, rerun the Online Installer to restore them; release startup uses only verified BITIG-managed tools.
- If yt-dlp is reported as unsafe or unverifiable, install media tool updates in About & Tools and restart BITIG.
- If saved media cannot play, use SHOW IN FOLDER to open it in another player, or EXPORT to save a copy.
- A migration failure is logged by the Rust backend and prevents startup rather than opening a partially migrated library.
- If the frontend builds but desktop startup fails, confirm the Tauri platform prerequisites and WebView runtime for the current OS.
