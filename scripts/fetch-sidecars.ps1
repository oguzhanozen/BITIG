param(
    [string]$TargetTriple = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
. (Join-Path $PSScriptRoot "sha256.ps1")

if ($TargetTriple -notlike "*windows*") {
    throw "Automatic sidecar fetching currently supports Windows targets only."
}

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$binaryRoot = Join-Path $projectRoot "src-tauri\binaries"
$manifestPath = Join-Path $binaryRoot "manifest.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$entries = @($manifest.artifacts | Where-Object { $_.target -eq $TargetTriple })

if ($entries.Count -ne 3) {
    throw "Expected three manifest entries for $TargetTriple."
}

$installedSetIsValid = $true
foreach ($entry in $entries) {
    $installedPath = Join-Path $binaryRoot "$($entry.name)-$TargetTriple.exe"
    if (-not (Test-Path -LiteralPath $installedPath -PathType Leaf)) {
        $installedSetIsValid = $false
        break
    }
    $installedHash = Get-Sha256 -LiteralPath $installedPath
    if ($installedHash -ne $entry.sha256.ToLowerInvariant()) {
        $installedSetIsValid = $false
        break
    }
}

if ($installedSetIsValid) {
    Write-Output "The pinned sidecars are already installed and verified for $TargetTriple."
    exit 0
}

$temporaryBase = [IO.Path]::GetTempPath().TrimEnd([IO.Path]::DirectorySeparatorChar)
$temporaryRoot = Join-Path $temporaryBase "bitig-sidecars-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null

try {
    $ytDlp = @($entries | Where-Object { $_.name -eq "yt-dlp" }) | Select-Object -First 1
    if (-not $ytDlp) {
        throw "The manifest has no yt-dlp entry for $TargetTriple."
    }
    $ytDlpPath = Join-Path $temporaryRoot "yt-dlp-$TargetTriple.exe"
    Invoke-WebRequest -Uri $ytDlp.sourceUrl -OutFile $ytDlpPath -Headers @{ "User-Agent" = "BITIG-sidecar-fetch" }

    $ffmpegEntries = @($entries | Where-Object { $_.name -in @("ffmpeg", "ffprobe") })
    $ffmpegUrls = @($ffmpegEntries.sourceUrl | Select-Object -Unique)
    if ($ffmpegEntries.Count -ne 2 -or $ffmpegUrls.Count -ne 1) {
        throw "The FFmpeg manifest entries are incomplete or use different archives."
    }

    $archivePath = Join-Path $temporaryRoot "ffmpeg.zip"
    $expandedPath = Join-Path $temporaryRoot "ffmpeg-expanded"
    Invoke-WebRequest -Uri $ffmpegUrls[0] -OutFile $archivePath -Headers @{ "User-Agent" = "BITIG-sidecar-fetch" }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $expandedPath

    foreach ($name in @("ffmpeg", "ffprobe")) {
        $source = Get-ChildItem -LiteralPath $expandedPath -Recurse -File -Filter "$name.exe" | Select-Object -First 1
        if (-not $source) {
            throw "$name.exe was not found in the pinned FFmpeg archive."
        }
        Copy-Item -LiteralPath $source.FullName -Destination (Join-Path $temporaryRoot "$name-$TargetTriple.exe")
    }

    foreach ($entry in $entries) {
        $fileName = "$($entry.name)-$TargetTriple.exe"
        $candidate = Join-Path $temporaryRoot $fileName
        $actualHash = Get-Sha256 -LiteralPath $candidate
        if ($actualHash -ne $entry.sha256.ToLowerInvariant()) {
            throw "Checksum mismatch for $fileName."
        }
    }

    foreach ($entry in $entries) {
        $fileName = "$($entry.name)-$TargetTriple.exe"
        Copy-Item -LiteralPath (Join-Path $temporaryRoot $fileName) -Destination (Join-Path $binaryRoot $fileName) -Force
        Write-Output "Installed verified $fileName"
    }
}
finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        $resolvedTemporaryRoot = (Resolve-Path -LiteralPath $temporaryRoot).Path
        if (-not $resolvedTemporaryRoot.StartsWith($temporaryBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Temporary cleanup target escaped the system temporary directory."
        }
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}
