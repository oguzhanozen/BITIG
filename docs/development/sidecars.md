# Sidecar layout

Development can use `yt-dlp`, `ffmpeg`, and `ffprobe` from `PATH`, or the explicit `BITIG_YTDLP_PATH`, `BITIG_FFPROBE_PATH`, `BITIG_FFMPEG_PATH`, and `BITIG_FFMPEG_DIR` environment variables.

The Windows x64 release configuration builds an Online Installer with
`bundle.externalBin: []`; media tools are not embedded in setup.exe. It uses a
custom NSIS template based on the official Tauri CLI 2.11.4 template and the
hooks in `src-tauri/nsis/installer-hooks.nsh`. Earlier bundled installers retain
their original behavior. Development/reference artifacts retain Tauri's
target-triple naming convention under `src-tauri/binaries/`:

```text
yt-dlp-<target-triple>[.exe]
ffmpeg-<target-triple>[.exe]
ffprobe-<target-triple>[.exe]
```

The reference manifest covers Windows x64. The executable files are intentionally excluded from Git because the FFmpeg files exceed GitHub's single-file limit. Restore the exact pinned development binaries and verify their hashes with:

```text
npm run sidecars:fetch
```

Planned targets are macOS Intel, macOS Apple Silicon, and Linux x64. Each artifact must also have an entry in `src-tauri/binaries/manifest.json` with its upstream version, source URL, license, target, and lowercase SHA-256 checksum. Do not commit binaries without this provenance.

For an Online Installer build, run:

```text
npm run tauri:build
```

`tauri:build` verifies provenance and notices with `sidecars:verify -- -SkipBinaries`, applies `src-tauri/tauri.release.conf.json`, and runs `installer:verify` against the actual NSIS package. Local media binaries are not required for this release path. Default `sidecars:verify` still checks pinned development binaries and fails closed when a binary, provenance field, or checksum is missing; `-SkipBinaries` skips only the binary presence/hash checks. Ordinary development remains unbundled. Both analysis and downloads pass `--ignore-config`, preventing a user's global yt-dlp format rules from changing BITIG behavior.

## Online Installer Bootstrap and Preference Handoff

Windows x64 supports these entry points:

```text
bitig.exe --installer-bootstrap --auto-update=enabled|disabled --defer-preferences
bitig.exe --installer-commit-preferences --auto-update=enabled|disabled
```

Pass exactly one of `--auto-update=enabled` or `--auto-update=disabled`.
The original two-argument bootstrap command without `--defer-preferences`
remains supported and writes the preference immediately after tool readiness.
NSIS uses the deferred form instead: it provisions tools without changing an
existing preference, then commits the selected preference in the final
post-install hook after successful application file/registration writes.
The commit command only atomically writes the preference; it does not install
or update tools.
Bootstrap runs before Tauri/WebView2, AppState, or SQLite initialization. It
uses the current user's LocalAppData and the application identifier to resolve
the same application data root as normal startup. It accepts no arbitrary
frontend or command-line storage path. Exit codes are 0 for success, 1 for a
provisioning/preference error, and 2 for invalid arguments.

Bootstrap first discovers complete verified BITIG installations under
`AppLocalData/tools/`. A reusable set returns before any GitHub catalog request,
so this path works offline with zero network requests. Neither bundled files
nor independent tools on system PATH are bootstrap inputs. Normal development
overrides described above remain a separate startup behavior.

When no valid managed set exists, bootstrap resolves yt-dlp and GyanD FFmpeg
essentials releases using the existing GitHub catalog validators. Downloads
require HTTPS, the expected repository/asset path, bounded sizes, and a
GitHub-published SHA-256 digest. FFmpeg and ffprobe are extracted together from
one validated ZIP with path, entry, and extracted-size checks. Files are staged
under `temp/<UUID>/installation`, hashed, recorded in `manifest.json`, and
verified again before atomic rename into `tools/install-<UUIDv7>`. Failed
provisioning does not replace existing installations. Required tool failure
prevents the installer from reporting successful completion.

The final commit atomically writes `AppLocalData/installer-preferences.json`
with `schemaVersion: 1` and `autoUpdateTools: "enabled"` or `"disabled"`.
The T3.5 frontend preference
initializer gives an existing localStorage `enabled`/`disabled` choice priority.
Only an undecided application preference can import the installer choice;
invalid/unreadable data or an unconfirmed storage write never enables updates.
With no saved installer choice, the existing in-app consent flow remains.
Automatic updates default to off and concern media tools, not BITIG itself.

Required provisioning is independent of optional automatic-update permission.
T3.4 physical old-installation deletion remains disabled: another BITIG process
may still use an older executable. Bootstrap does not enable cleanup; retained
installations can increase disk usage. Per-install manifests record versions
and binary SHA-256 values, not upstream commits or build configuration. See
`THIRD-PARTY-SOURCES.md` for the version-specific source lookup method.

## Installer Terms and Tauri 2 NSIS Integration Notes

`bundle.licenseFile` points to `../licenses/INSTALLER-TERMS.txt`, the English
plain-text document prepared in T2.4. Its three parts cover use, third-party
licenses, and informational local data/network disclosures. The acceptance
label is:

```text
I accept the BITIG Terms of Use and have read the Third-Party Software Notices.
```

The native MUI license page enables `MUI_LICENSEPAGE_CHECKBOX`, which starts
unchecked and blocks continuation until checked. The leave callback verifies
the actual checkbox state. This is separate from automatic tool updates,
does not serve as KVKK explicit consent, and does not restrict MIT or other
open-source rights. Installer initialization rejects `/S`, `/P`, `/s`, and `/p`
with exit code 2 before bootstrap or application writes. A silent/passive flag
alone is not consent; unattended installation is unsupported.

The component screen uses these exact labels:

```text
[x] yt-dlp — Video and audio downloading
[x] FFmpeg + ffprobe — Media processing and analysis
[ ] Automatically update media tools
```

The first two components are required, selected, and locked; they cannot be
deselected. Automatic updates are optional and initially unchecked. Screen
information:

> BITIG requires yt-dlp, FFmpeg, and ffprobe for its core media features. Setup reuses existing verified BITIG tools or securely downloads and verifies missing components. Automatic tool updates are optional.

Tauri 2 uses `bundle.licenseFile` for the license document. NSIS customization uses
`bundle.windows.nsis.installerHooks` for hook macros and
`bundle.windows.nsis.template` for a custom template. There is no
`bundle.windows.nsis.license` field. Setting the license file alone does not
enable explicit acceptance: BITIG's template/hooks configure the native MUI
checkbox and add the media component page. The native checkbox control ID is
tied to the inspected NSIS 3.11 implementation and must be retested on upgrades.

The first installer section, `BitigPrepare`, extracts the new packaged BITIG
executable into `$PLUGINSDIR/bitig-bootstrap.exe` and invokes its deferred
bootstrap. It never uses a previously installed BITIG executable. This section
runs before WebView2 setup and before uninstalling or replacing the existing
application. Failures offer Retry/Cancel; cancellation aborts with nonzero exit
status and leaves the previous application and preference untouched. Verified
managed tools produced during preparation may remain for reuse.

After successful tool preparation and WebView2 setup, the pre-install hook
runs a remembered previous Tauri uninstaller with `/P /UPDATE` to suppress its
user-data deletion option. Application files/resources are then installed and
the post-install hook runs `--installer-commit-preferences` using the temporary
new executable. Later file, registry, or preference-write failures are reported;
there is no promise of complete rollback after the old uninstaller has run.

Official references: [Tauri 2 configuration](https://v2.tauri.app/reference/config/)
and [Windows installer customization](https://v2.tauri.app/distribute/windows-installer/).

The current resource mapping puts repository `licenses/*` into installation
`licenses/*`, `LICENSE` into `licenses/BITIG-MIT.txt`, and
`THIRD-PARTY-SOURCES.md` into `licenses/THIRD-PARTY-SOURCES.md`. These installed
paths are relative to the application installation directory, not AppLocalData.
`installer:verify` uses 7-Zip to list and extract the actual NSIS package,
requires all eleven license/notice/source documents exactly once, compares
their hashes with repository files, and verifies both packaged BITIG copies
match the new build. It rejects embedded media executables and application
updater artifacts, checks bootstrap section ordering and configured hooks, and
requires an empty `externalBin`. Supply `7z.exe` on PATH, `BITIG_7ZIP_PATH`, or
the script's `-SevenZipPath` argument. These checks do not prove the installed
documents are readable or complete an interactive installation test.

`npm run test:nsis` exercises native NSIS controls, page boundaries, Back/Next
state, and bootstrap failure/retry handling using inert executables and scratch
directories. It does not run the actual BITIG setup locally. In CI, the optional
`-InstallerPath` argument additionally verifies actual unattended rejection on
a fresh GitHub-hosted runner; it is refused outside that isolated environment.
Complete first-install, WebView2, reinstall, and media-operation testing still
requires an isolated Windows environment. Do not run those tests against a
real user's `%LOCALAPPDATA%/app.bitig.desktop` directory.

The current UI has no source/license links and
bootstrap does not fetch version-matched license notices or source materials.
Publishing the Online Installer requires a verified way to keep those notices,
the actual corresponding source, dependency sources, and build information
accessible for every delivered version; baseline documents alone do not prove
that obligation fulfilled.

Real-installer validation must cover text encoding, Unicode characters, line
endings, readability, interactive acceptance, rejected silent/passive modes,
checkbox defaults, bootstrap failure/retry, and preference handoff. The release
workflow retains corresponding-source archives and checksums, runs package
verification, and uses `installer:verify -- -ReleaseGate` before publication.
The gate deliberately fails on an unresolved `[RELEASE BLOCKER` placeholder
in INSTALLER-TERMS.txt. The distributor identity/contact placeholder, legal
review, and version-matched notice/source access remain publication blockers;
technical package checks do not resolve them.

## Startup tool selection

Release builds select only verified `AppLocalData/tools/install-*` installations. Legacy packaged executables beside BITIG are left untouched and ignored; release startup does not hash or select them and never falls back to `PATH` or environment overrides. Without a verified managed set, startup returns the existing tool error; required provisioning is the installer's job. Selection is independent for yt-dlp and the FFmpeg/ffprobe pair. FFmpeg and ffprobe always come from the same verified installation; yt-dlp may come from another directory. Reported installed versions match the selected files.

Every cached candidate is validated through its existing manifest and all three SHA-256 checks. Invalid installations are ignored. Candidates are ranked by tool version, rather than installation UUID. Numeric releases take precedence over unorderable cache versions; UUID order remains a deterministic fallback for equal or unorderable candidates.

Comparison uses complete, non-overflowing dotted numeric components (including yt-dlp's date-based nightly build numbers), ignores trailing zero components, and recognizes the pinned FFmpeg ` essentials build` label and an optional `v` prefix. Other prerelease, nightly, or Git suffixes are not assumed to be comparable with releases.

Discovery performs no network requests, downloads, or old-installation cleanup, so it works offline.

Development retains its existing behavior: when any development tool path is outside the executable directory, a verified cache still takes precedence; without a cache, the environment overrides and `PATH` fallback above remain unchanged. When all three paths are beside the executable (`are_bundled_in`), they are checked against the reference manifest: a cached group replaces them only if newer or if the local group fails verification. Equal or incomparable versions prefer the verified local group. `BITIG_FFMPEG_DIR` still takes precedence over `BITIG_FFMPEG_PATH`.

## In-app update checks

FFmpeg checks and installation both resolve `https://api.github.com/repos/GyanD/codexffmpeg/releases/latest` through the same release validator. A candidate requires a comparable release tag and exactly one matching `essentials_build.zip` asset, with its exact GitHub repository/release/file path, HTTPS URL, bounded nonzero size, and SHA-256 digest. Checks fetch metadata only; missing or invalid metadata produces `CheckFailed` with a safe message. yt-dlp retains its existing GitHub release source.

Startup, status checks, and installation share the numeric comparison policy above. Only a strictly newer, comparable version is offered or installed; equal versions (including trailing zero components) and older versions need no update. Unorderable versions produce `CheckFailed` during checks and are not installed as upgrades. If neither tool needs an update, installation returns without downloading or staging files and does not require a restart. Actual updates retain verified downloads, temporary staging, atomic installation, the FFmpeg/ffprobe pair, and the restart requirement. Automatic startup checks require the separate opt-in below.

## Optional automatic media tool updates

When neither the application nor installer has supplied a valid choice, the frontend asks about third-party media tools before the first Quick Tour. Automatic updates default to off. `bitig.toolUpdates.auto` stores `undecided`, `enabled`, or `disabled` in localStorage; missing data is undecided, while invalid or unreadable data keeps updates off. An existing application choice takes priority over `installer-preferences.json`, as described above. Enablement requires a successful, read-back-confirmed write. Declining or dismissing the consent dialog saves disabled. Storage errors are shown without pretending the choice was saved. The preference can be changed in About & Tools, independently of installer terms acceptance.

With explicit enabled consent, App starts at most one automatic attempt per frontend session after its progress listener is ready. The session guard is set immediately before invoking the existing `install_tool_updates` command; React StrictMode setup/cleanup replay does not consume it before an invocation or duplicate a running operation. Manual installation uses the same runner and backend lock. Only an automatic Conflict is silently skipped; other errors remain visible and do not create a retry loop. Opting out during installation affects future attempts and does not cancel the backend owner task.

App owns progress and results while About & Tools is closed. Events are accepted only for the active operation ID, and pending or obsolete subscriptions are disposed on cleanup. Completion comes from the invoke result, followed by `get_tool_status(false)` with no second GitHub catalog request. A no-op stays quiet; an installed update offers an explicit restart without restarting automatically. Startup remains nonblocking and verified managed tools remain usable offline. No telemetry, scheduler, hot-swap, or physical cache deletion is introduced. Since old installations remain protected, automatic updates can increase disk usage over time.

## Old tool installation retention

Startup inspects a retention plan after discovery and creation of the update service, before download services start. It reuses discovery's verified cache inventory without another binary hash pass. Discovery and retention share the same version ranking: keep the first two verified installations for each group (primary and fallback), plus every installation referenced by active tool paths. FFmpeg and ffprobe remain one group. With no unprotected candidate, no additional filesystem scan is needed.

Only direct, canonically named `install-<UUIDv7>` children with the four expected regular files can become deferred candidates. Unknown entries, corrupt or incomplete installations, unexpected contents, links, and Windows reparse points remain untouched. Inspection errors are logged and never fail startup. The existing update mutex excludes same-process updates and remains owned by the inspection worker if its caller is cancelled.

**Automatic physical deletion is disabled.** Older BITIG processes retain executable paths without holding a lifetime lock; a new standard-library file lock cannot coordinate those older processes. Even structurally valid, SHA-256-verified candidates may still be used later by another process. Inspection therefore only logs deferred candidates and does not remove files, directories, or partial installations. Discovery snapshots and canonical path checks are not deletion authority and do not remove TOCTOU risks. Physical cleanup requires a separately proven process-lifetime and filesystem protocol covering every participating version. Bundled tools and user media are never cleanup targets; no cleanup runs after installation or during shutdown.
