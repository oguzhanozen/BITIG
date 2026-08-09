# BITIG

BITIG is a local-first desktop media downloader and organized media library built with Tauri 2, Rust, React, TypeScript, and SQLite.

The v0.1 product flow is URL analysis, normalized video/audio selection, managed download and verification, then playback and organization in nested virtual folders. BITIG is for media the user owns or has permission to download. It does not bypass DRM or access controls.

## Prerequisites

- Node.js 24+ and npm 11+
- Rust 1.88+ with the platform prerequisites required by Tauri 2
- Development sidecars available through `PATH`, the environment variables described in `docs/development/sidecars.md`, or `npm run sidecars:fetch`

End users should not need a separate Python, yt-dlp, FFmpeg, or ffprobe installation; release builds will bundle platform-specific sidecars.

## Development

```text
npm install
npm run tauri dev
```

Frontend-only development is available with `npm run dev`, although Tauri commands require the desktop shell.

## Release build

Windows x64 release sidecars are pinned and checksum-verified through `src-tauri/binaries/manifest.json`. Executables are intentionally excluded from Git because the FFmpeg files exceed GitHub's single-file limit. Run `npm run sidecars:fetch` and then `npm run tauri:build` to create the NSIS installer. Other target platforms must provide their matching verified binaries as described in `docs/development/sidecars.md`.

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

Tauri resolves the OS-specific AppLocalData directory. BITIG creates `library.db`, `media/`, `thumbnails/`, `temp/`, and `logs/` below that root. UI folders are SQLite records; moving or renaming media does not rename the internal media file.

## Font and licensing

BITIG source code is released under the MIT License; see `LICENSE`.

Press Start 2P is bundled under `src/assets/fonts/` so the desktop UI works offline. Its SIL Open Font License is stored in `licenses/PressStart2P-OFL.txt`. The font asset and license are sourced from the official Google Fonts repository.

Windows releases currently bundle yt-dlp 2026.07.04 and the Gyan FFmpeg 9.0 essentials build. Their provenance, exact source references, and hashes are recorded in the sidecar manifest. Corresponding license texts, third-party notices, FFmpeg source/build information, and generated Rust notices are stored under `licenses/` and included in the Windows installer.

## Troubleshooting

- A missing sidecar error means the current platform binary names do not match the target triple; see `docs/development/sidecars.md`.
- A migration failure is logged by the Rust backend and prevents startup rather than opening a partially migrated library.
- If the frontend builds but desktop startup fails, confirm the Tauri platform prerequisites and WebView runtime for the current OS.
