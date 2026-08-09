# Sidecar layout

Development can use `yt-dlp`, `ffmpeg`, and `ffprobe` from `PATH`, or the explicit `BITIG_YTDLP_PATH`, `BITIG_FFPROBE_PATH`, `BITIG_FFMPEG_PATH`, and `BITIG_FFMPEG_DIR` environment variables.

Release artifacts use Tauri's target-triple naming convention under `src-tauri/binaries/`:

```text
yt-dlp-<target-triple>[.exe]
ffmpeg-<target-triple>[.exe]
ffprobe-<target-triple>[.exe]
```

The current release manifest covers Windows x64. The executable files are intentionally excluded from Git because the FFmpeg files exceed GitHub's single-file limit. Restore the exact pinned binaries and verify their hashes with:

```text
npm run sidecars:fetch
```

Planned targets are macOS Intel, macOS Apple Silicon, and Linux x64. Each artifact must also have an entry in `src-tauri/binaries/manifest.json` with its upstream version, source URL, license, target, and lowercase SHA-256 checksum. Do not commit binaries without this provenance.

Before a Windows x64 release, fetch the three target-specific executables, then run:

```text
npm run sidecars:fetch
npm run sidecars:verify
npm run tauri:build
```

`sidecars:verify` fails closed when a file, provenance field, or checksum is missing. `tauri:build` applies `src-tauri/tauri.release.conf.json` and creates a Windows NSIS installer; ordinary development remains unbundled. Development builds may use the paths above. Release builds accept only the three bundled executables beside BITIG and verify their SHA-256 hashes before startup; they never fall back to `PATH` or environment overrides. Both analysis and downloads pass `--ignore-config`, preventing a user's global yt-dlp format rules from changing BITIG behavior.
