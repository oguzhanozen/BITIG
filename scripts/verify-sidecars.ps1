param(
    [string]$TargetTriple = ""
)

$ErrorActionPreference = "Stop"

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
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$extension = if ($TargetTriple -like "*windows*") { ".exe" } else { "" }

foreach ($name in @("yt-dlp", "ffmpeg", "ffprobe")) {
    $fileName = "$name-$TargetTriple$extension"
    $path = Join-Path $binaryRoot $fileName
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Missing release sidecar: $fileName"
    }

    $entry = @($manifest.artifacts | Where-Object {
        $_.name -eq $name -and $_.target -eq $TargetTriple
    }) | Select-Object -First 1
    if (-not $entry) {
        throw "No manifest entry for $name on $TargetTriple"
    }
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

    $actualHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $entry.sha256.ToLowerInvariant()) {
        throw "Checksum mismatch for $fileName"
    }
    Write-Output "$fileName $actualHash"
}

foreach ($licenseFile in @(
    "FFmpeg-GPLv3.txt",
    "FFmpeg-SOURCE-AND-BUILD-INFO.txt",
    "FFmpeg-UPSTREAM-LICENSE.md",
    "Node-THIRD-PARTY-NOTICES.txt",
    "PressStart2P-OFL.txt",
    "Rust-THIRD-PARTY-NOTICES.txt",
    "yt-dlp-THIRD-PARTY-LICENSES.txt",
    "yt-dlp-Unlicense.txt"
)) {
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

Write-Output "Sidecars verified for $TargetTriple"
