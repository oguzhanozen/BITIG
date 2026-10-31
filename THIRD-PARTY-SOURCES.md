# Third-party corresponding source

BITIG's Windows x64 release configuration builds an Online Installer without
embedded media tools. The versions below remain the development/reference
baseline, with exact binary SHA-256 and source revisions recorded in
`src-tauri/binaries/manifest.json`. That manifest describes reference binaries,
not subsequently downloaded tools. Previously published bundled installers
retain their original behavior; release startup built from current source uses
only verified BITIG-managed installations under AppLocalData.

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
identifies GPL-2.0-or-later components, including mutagen. Upstream identifies
the combined PyInstaller executable as GPLv3+, so BITIG records and distributes
the aggregate executable under GPL-3.0-or-later while preserving the distinct
Unlicense status of the yt-dlp core. The baseline upstream notices and core
license are preserved under `licenses/` and included by the Online
installer resource mapping.

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
- Preserved upstream licensing and external-library record: `licenses/FFmpeg-UPSTREAM-LICENSE.md` (revision identified in its header; also consult the actual build's source revision)
- License text: `licenses/FFmpeg-GPLv3.txt`

ffmpeg and ffprobe come from the same archive and source revision. BITIG does
not patch either executable.

GyanD is the selected Windows binary distributor, not the FFmpeg development
project. The [GyanD 9.0.1 release](https://github.com/GyanD/codexffmpeg/releases/tag/9.0.1)
explicitly links upstream FFmpeg commit `bf1b838f2a`, which resolves to the full
revision recorded above. The provider repository's commit is distinct from
the FFmpeg source commit. The recorded `n9.0.1` baseline is not a rule for
deriving source revisions from future GyanD tag names. The inspected ffmpeg
and ffprobe binaries both report `--enable-gpl`, `--enable-version3`, and
GPL-3.0-or-later; later builds require their own licensing checks.

## Online Installer and In-App Updated Tool Versions

The bootstrap first reuses verified BITIG-managed tools, without any network
request when a complete valid set is available. Otherwise it downloads yt-dlp
from the yt-dlp project's GitHub Releases and the FFmpeg essentials archive
from GyanD/codexffmpeg GitHub Releases. HTTPS, SHA-256, size limits, ZIP safety
checks, and atomic installation apply. System PATH installations are not
bootstrap inputs. Automatic media tool updates are optional and off by
default; accepting installer terms does not enable them.

Downloaded installations live under `AppLocalData/tools/install-<UUIDv7>/`.
Each `manifest.json` records `ytDlpVersion`, `ytDlpSha256`, `ffmpegVersion`,
`ffmpegSha256`, and `ffprobeSha256`, alongside its schema version. It does not
record the upstream commit, source URLs, or build configuration. About & Tools
shows the selected yt-dlp and FFmpeg build versions after restart; an update
requires restarting BITIG before the newly installed tools are used. ffprobe
is part of the same FFmpeg build, without its own UI version row. yt-dlp and
the FFmpeg/ffprobe pair may be selected from different managed installations;
match the displayed versions to the relevant manifests. Legacy packaged media
tools are left untouched and ignored by current release startup; their
reference provenance above remains available for older distributions.

To obtain sources for the installed versions:

1. For yt-dlp, select that version on the [official Releases page](https://github.com/yt-dlp/yt-dlp/releases).
   Follow the release tag/commit to its source tree and downloadable source
   archive. Read that revision's `LICENSE` and `THIRD_PARTY_LICENSES.txt` for
   component licenses and upstream corresponding-source instructions. The
   core source archive alone is not all bundled dependencies' source code.
2. For FFmpeg, select the matching build on the [GyanD Releases page](https://github.com/GyanD/codexffmpeg/releases).
   Open its explicitly reported upstream FFmpeg Source commit/tag link. Use
   that actual revision's source tree or archive; never assume that `n` plus
   the provider tag identifies the build's source. If the provider does not
   establish the exact relation, obtain verified provenance before claiming
   that a source archive corresponds to the binary.
3. Run the selected `ffmpeg.exe -version` and `ffmpeg.exe -L` for configuration
   and license information; use the same commands for `ffprobe.exe`. Consult
   the [provider's build and dependency information](https://www.gyan.dev/ffmpeg/builds/)
   and the relevant upstream license/corresponding-source documentation for
   external libraries and build materials. An FFmpeg upstream archive alone
   does not establish complete corresponding source for the entire build.

The source archives and hashes above apply only to the reference binaries.
Preserved local notices must not be represented as version-matched notices
for every later download. The current UI has no source/license links, and the
bootstrap does not collect updated notices or corresponding source. Before
publishing the Online Installer, the release process must ensure accessible,
version-matched notices and corresponding source/build materials under the
applicable licenses. Downloading tools online does not remove these duties.

BITIG's source remains MIT-licensed. Tools run as separate executable
subprocesses; that technical separation is not a blanket determination of GPL
exemption or of the legal status of every distribution. Installer terms do
not restrict rights granted by MIT or third-party licenses.

## Release requirement

The release workflow verifies reference provenance and notices with
`-SkipBinaries`, builds the Online Installer, and inspects the actual NSIS
package with 7-Zip. The package must contain all eleven license/notice/source
documents with matching repository hashes, and no media executables or
application updater artifacts. Publication uses `-ReleaseGate`, which fails
while the installer terms contain a `[RELEASE BLOCKER` placeholder. These
technical checks do not establish legal compliance or dynamic source coverage.
The workflow retains the reference corresponding-source archives and publishes
a SHA-256 checksum file covering release artifacts. A release maintainer must also
attach or otherwise keep reachable the exact source archives linked above. If
an upstream source becomes unavailable, that release must not be rebuilt or
redistributed until an exact mirror is available and documented here.
