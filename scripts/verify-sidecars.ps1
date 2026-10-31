[CmdletBinding()]
param(
    [string]$TargetTriple = "",
    [switch]$SkipBinaries,
    [switch]$ReleaseGate,
    [switch]$VerifyInstallerPackage,
    [string]$InstallerPath = "",
    [string]$ScriptPath = "",
    [string]$SevenZipPath = ""
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "sha256.ps1")

if ([string]::IsNullOrWhiteSpace($TargetTriple)) {
    $hostLine = & rustc -vV | Where-Object { $_ -like "host:*" } | Select-Object -First 1
    if (-not $hostLine) {
        throw "Could not determine the Rust host target. Pass -TargetTriple explicitly."
    }
    $TargetTriple = ($hostLine -split ":", 2)[1].Trim()
}

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$binaryRoot = Join-Path $projectRoot "src-tauri\binaries"
$manifestPath = Join-Path $binaryRoot "manifest.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schemaVersion -ne 2 -or @($manifest.artifacts).Count -eq 0) {
    throw "Unsupported or empty sidecar manifest"
}
$extension = if ($TargetTriple -like "*windows*") { ".exe" } else { "" }

foreach ($name in @("yt-dlp", "ffmpeg", "ffprobe")) {
    $fileName = "$name-$TargetTriple$extension"
    $path = Join-Path $binaryRoot $fileName
    $entries = @($manifest.artifacts | Where-Object {
        $_.name -eq $name -and $_.target -eq $TargetTriple
    })
    if ($entries.Count -ne 1) {
        throw "Expected exactly one manifest entry for $name on $TargetTriple"
    }
    $entry = $entries[0]
    $requiredFields = @(
        "version",
        "sourceUrl",
        "sourceTag",
        "sourceCommit",
        "sourceCodeUrl",
        "sourceArchiveUrl",
        "sourceArchiveSha256",
        "license",
        "sha256"
    )
    if ($name -eq "yt-dlp") {
        $requiredFields += @("thirdPartyNoticesUrl", "coreLicense", "licenseNote")
        if ($entry.license -ne "GPL-3.0-or-later" -or $entry.coreLicense -ne "Unlicense") {
            throw "yt-dlp must record the executable aggregate as GPL-3.0-or-later and the core as Unlicense"
        }
    } else {
        $requiredFields += @("buildInformationUrl", "buildProviderTag", "buildProviderCommit")
    }
    foreach ($field in $requiredFields) {
        if ([string]::IsNullOrWhiteSpace($entry.$field)) {
            throw "Manifest entry for $name on $TargetTriple is missing $field"
        }
    }
    foreach ($field in @("sha256", "sourceArchiveSha256")) {
        if ($entry.$field -cnotmatch '^[a-f0-9]{64}$') { throw "Invalid $field for $name" }
    }
    foreach ($field in @("sourceCommit") + $(if ($name -ne "yt-dlp") { "buildProviderCommit" })) {
        if ($entry.$field -cnotmatch '^[a-f0-9]{40}$') { throw "Invalid $field for $name" }
    }
    foreach ($field in @("sourceUrl", "sourceCodeUrl", "sourceArchiveUrl") + $(if ($name -eq "yt-dlp") { "thirdPartyNoticesUrl" } else { "buildInformationUrl" })) {
        $uri = $null
        if (-not [Uri]::TryCreate($entry.$field, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne "https") {
            throw "Invalid HTTPS $field for $name"
        }
    }
    if (-not $SkipBinaries) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing release sidecar: $fileName" }
        $actualHash = Get-Sha256 -LiteralPath $path
        if ($actualHash -ne $entry.sha256) { throw "Checksum mismatch for $fileName" }
        Write-Output "$fileName $actualHash"
    }
}

$licenseFiles = @(
    "INSTALLER-TERMS.txt",
    "FFmpeg-GPLv3.txt",
    "FFmpeg-SOURCE-AND-BUILD-INFO.txt",
    "FFmpeg-UPSTREAM-LICENSE.md",
    "Node-THIRD-PARTY-NOTICES.txt",
    "PressStart2P-OFL.txt",
    "Rust-THIRD-PARTY-NOTICES.txt",
    "yt-dlp-THIRD-PARTY-LICENSES.txt",
    "yt-dlp-Unlicense.txt"
)
foreach ($licenseFile in $licenseFiles) {
    $licensePath = Join-Path $projectRoot "licenses\$licenseFile"
    if (-not (Test-Path -LiteralPath $licensePath -PathType Leaf)) {
        throw "Missing bundled license notice: $licenseFile"
    }
}

$sourceLicense = Join-Path $projectRoot "LICENSE"
if (-not (Test-Path -LiteralPath $sourceLicense -PathType Leaf)) {
    throw "Missing BITIG MIT source license"
}

$sourceIndex = Join-Path $projectRoot "THIRD-PARTY-SOURCES.md"
if (-not (Test-Path -LiteralPath $sourceIndex -PathType Leaf)) {
    throw "Missing corresponding-source index: THIRD-PARTY-SOURCES.md"
}

if ($ReleaseGate -and (Get-Content -LiteralPath (Join-Path $projectRoot "licenses/INSTALLER-TERMS.txt") -Raw -Encoding UTF8).Contains("[RELEASE BLOCKER")) {
    throw "Unresolved RELEASE BLOCKER in licenses/INSTALLER-TERMS.txt; publication is prohibited."
}

if ($VerifyInstallerPackage) {
    $configuration = Get-Content -LiteralPath (Join-Path $projectRoot "src-tauri/tauri.release.conf.json") -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($null -eq $configuration.bundle.externalBin -or @($configuration.bundle.externalBin).Count -ne 0) {
        throw "Online Installer must not bundle external binaries"
    }
    if ($configuration.bundle.licenseFile -ne "../licenses/INSTALLER-TERMS.txt" -or
        $configuration.bundle.windows.nsis.installMode -ne "currentUser" -or
        -not $configuration.bundle.windows.nsis.template -or -not $configuration.bundle.windows.nsis.installerHooks -or
        $configuration.bundle.createUpdaterArtifacts) {
        throw "Invalid Online Installer license, NSIS, current-user, or updater configuration"
    }
    if (-not $InstallerPath -or -not $ScriptPath) {
        $metadata = & cargo metadata --no-deps --locked --offline --format-version 1 --manifest-path (Join-Path $projectRoot "src-tauri/Cargo.toml") | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or -not $metadata.target_directory) { throw "Could not locate Cargo build output" }
        $releaseRoot = Join-Path $metadata.target_directory "release"
        if (-not $InstallerPath) {
            $installers = @(Get-ChildItem -LiteralPath (Join-Path $releaseRoot "bundle/nsis") -File -Filter "BITIG_*_x64-setup.exe")
            if ($installers.Count -ne 1) { throw "Expected exactly one Windows x64 installer; pass -InstallerPath explicitly." }
            $InstallerPath = $installers[0].FullName
        }
        if (-not $ScriptPath) {
            $scripts = @(Get-ChildItem -LiteralPath (Join-Path $releaseRoot "nsis") -Recurse -File -Filter "installer.nsi" | Where-Object {
                (Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8) -match '!define ARCH "x64"'
            })
            if ($scripts.Count -ne 1) { throw "Expected exactly one generated x64 NSIS script; pass -ScriptPath explicitly." }
            $ScriptPath = $scripts[0].FullName
        }
    }
    $InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
    $ScriptPath = (Resolve-Path -LiteralPath $ScriptPath).Path
    if (-not $SevenZipPath) {
        $command = Get-Command 7z.exe -ErrorAction SilentlyContinue
        if ($command) { $SevenZipPath = $command.Source }
        foreach ($candidate in @($env:BITIG_7ZIP_PATH, "${env:ProgramFiles}/7-Zip/7z.exe", (Join-Path $projectRoot "node_modules/.tmp/t23/7zip/portable/7z.exe"))) {
            if (-not $SevenZipPath -and $candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { $SevenZipPath = $candidate }
        }
    }
    if (-not $SevenZipPath -or -not (Test-Path -LiteralPath $SevenZipPath -PathType Leaf)) {
        throw "7-Zip is required to inspect the actual NSIS package; use -SevenZipPath or BITIG_7ZIP_PATH."
    }
    $listing = & $SevenZipPath l -slt $InstallerPath | Out-String
    if ($LASTEXITCODE -ne 0 -or $listing -notmatch '(?m)^Type = Nsis\r?$') { throw "Could not inspect a valid NSIS package" }
    $members = @([regex]::Matches($listing, '(?m)^Path = (.+)\r?$') | Select-Object -Skip 1 | ForEach-Object { $_.Groups[1].Value.Trim().Replace('\', '/') })
    foreach ($member in $members) {
        if ($member -match '(^|/)(yt-dlp|ffmpeg|ffprobe)(-[^/]*)?\.exe$') { throw "Package contains media executable: $member" }
        if ($member -match '(^|/)latest\.json$|\.exe\.sig$|\.nsis\.zip$') { throw "Package contains application updater artifact: $member" }
        if ($member -match '(^|/)\.\.(/|$)|^/|:') { throw "Unsafe package member: $member" }
    }
    foreach ($required in @("bitig.exe", '$PLUGINSDIR/bitig-bootstrap.exe') + @($licenseFiles + @("BITIG-MIT.txt", "THIRD-PARTY-SOURCES.md") | ForEach-Object { "licenses/$_" })) {
        if (@($members | Where-Object { $_ -ceq $required }).Count -ne 1) { throw "Missing or duplicate package file: $required" }
    }
    $script = Get-Content -LiteralPath $ScriptPath -Raw -Encoding UTF8
    $prepare = [regex]::Match($script, '(?m)^Section "?BitigPrepare"?\s*\r?$')
    $firstSection = [regex]::Match($script, '(?m)^Section\s')
    if (-not $prepare.Success -or $prepare.Index -ne $firstSection.Index -or $script -notmatch 'Call BitigProvisionTools') {
        throw "Bootstrap must run in the first installer section before WebView2 and application writes"
    }
    $hooksPath = (Resolve-Path -LiteralPath (Join-Path (Join-Path $projectRoot "src-tauri") $configuration.bundle.windows.nsis.installerHooks)).Path
    if (-not $script.Replace('\', '/').Contains($hooksPath.Replace('\', '/'))) { throw "Generated script does not include configured installer hooks" }
    $hooks = Get-Content -LiteralPath $hooksPath -Raw -Encoding UTF8
    foreach ($required in @('MUI_LICENSEPAGE_CHECKBOX', 'I accept the BITIG Terms of Use and have read the Third-Party Software Notices.', 'Function BitigProvisionTools', 'nsExec::ExecToLog', '--installer-bootstrap', '--defer-preferences', '--installer-commit-preferences', 'BITIG_INIT')) {
        if (-not $hooks.Contains($required)) { throw "Missing installer integration: $required" }
    }
    if ($hooks -notmatch 'File\s+"?/oname=\$PLUGINSDIR\\bitig-bootstrap\.exe"?\s+"\$\{MAINBINARYSRCPATH\}"') {
        throw "Bootstrap must be extracted from the new packaged executable"
    }
    foreach ($artifact in Get-ChildItem -LiteralPath (Split-Path $InstallerPath) -File) {
        if ($artifact.Name -match '^latest\.json$|\.exe\.sig$|\.nsis\.zip$') { throw "Unexpected application updater artifact: $($artifact.Name)" }
    }
    $mainSource = [regex]::Match($script, '(?m)^!define MAINBINARYSRCPATH "([^"]+)"\r?$')
    if (-not $mainSource.Success) { throw "Generated script has no main executable source" }
    $expectedMainHash = Get-Sha256 -LiteralPath $mainSource.Groups[1].Value
    $expectedBytes = [IO.File]::ReadAllBytes($mainSource.Groups[1].Value)
    $marker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
    $sourceText = [Text.Encoding]::ASCII.GetString($expectedBytes)
    $markerOffset = $sourceText.IndexOf($marker, [StringComparison]::Ordinal)
    if ($markerOffset -ge 0) {
        if ($sourceText.IndexOf($marker, $markerOffset + 1, [StringComparison]::Ordinal) -ge 0) { throw "Ambiguous Tauri bundle marker" }
        # Tauri CLI 2.11.4 patches this exact marker for NSIS, then restores the source binary.
        $nsisMarker = [Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS')
        [Buffer]::BlockCopy($nsisMarker, 0, $expectedBytes, $markerOffset, $nsisMarker.Length)
        $hasher = [Security.Cryptography.SHA256]::Create()
        try { $expectedMainHash = ([BitConverter]::ToString($hasher.ComputeHash($expectedBytes))).Replace('-', '').ToLowerInvariant() }
        finally { $hasher.Dispose() }
    }
    $temporaryBase = [IO.Path]::GetTempPath().TrimEnd([IO.Path]::DirectorySeparatorChar)
    $extractRoot = Join-Path $temporaryBase "bitig-package-check-$([Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $extractRoot | Out-Null
    try {
        & $SevenZipPath x -y "-o$extractRoot" $InstallerPath 'licenses/*' 'bitig.exe' '$PLUGINSDIR/bitig-bootstrap.exe' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Could not extract package verification files" }
        foreach ($file in @('bitig.exe', '$PLUGINSDIR/bitig-bootstrap.exe')) {
            if ((Get-Sha256 -LiteralPath (Join-Path $extractRoot $file)) -ne $expectedMainHash) { throw "Packaged executable does not match the new build: $file" }
        }
        foreach ($file in $licenseFiles + @("BITIG-MIT.txt", "THIRD-PARTY-SOURCES.md")) {
            $source = Join-Path $projectRoot "licenses/$file"
            if ($file -eq "BITIG-MIT.txt") { $source = $sourceLicense }
            if ($file -eq "THIRD-PARTY-SOURCES.md") { $source = $sourceIndex }
            if ((Get-Sha256 -LiteralPath (Join-Path $extractRoot "licenses/$file")) -ne (Get-Sha256 -LiteralPath $source)) {
                throw "Packaged license differs from the source document: $file"
            }
        }
    }
    finally {
        $resolvedExtraction = (Resolve-Path -LiteralPath $extractRoot).Path
        if (-not $resolvedExtraction.StartsWith($temporaryBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Package cleanup escaped the temporary directory" }
        Remove-Item -LiteralPath $resolvedExtraction -Recurse -Force
    }
    Write-Output "Online Installer package verified: $InstallerPath ($((Get-Item -LiteralPath $InstallerPath).Length) bytes)"
}

Write-Output "Sidecars verified for $TargetTriple"
