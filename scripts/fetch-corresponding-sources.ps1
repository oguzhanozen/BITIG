param(
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "sha256.ps1")

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $projectRoot "release\corresponding-source"
}
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

$manifestPath = Join-Path $projectRoot "src-tauri\binaries\manifest.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$uniqueSources = @($manifest.artifacts | Group-Object sourceCommit | ForEach-Object {
    $_.Group | Select-Object -First 1
})

foreach ($entry in $uniqueSources) {
    if ([string]::IsNullOrWhiteSpace($entry.sourceCommit) -or
        [string]::IsNullOrWhiteSpace($entry.sourceArchiveUrl) -or
        [string]::IsNullOrWhiteSpace($entry.sourceArchiveSha256)) {
        throw "Missing exact corresponding-source data for $($entry.name)"
    }
    $fileName = "$($entry.name)-source-$($entry.sourceCommit).tar.gz"
    $destination = Join-Path $outputRoot $fileName
    $needsDownload = $true
    if (Test-Path -LiteralPath $destination -PathType Leaf) {
        $existingHash = Get-Sha256 -LiteralPath $destination
        $needsDownload = $existingHash -ne $entry.sourceArchiveSha256.ToLowerInvariant()
    }
    if ($needsDownload) {
        Invoke-WebRequest -UseBasicParsing -Uri $entry.sourceArchiveUrl -OutFile $destination
    }
    if ((Get-Item -LiteralPath $destination).Length -le 0) {
        throw "Downloaded source archive is empty: $fileName"
    }
    $hash = Get-Sha256 -LiteralPath $destination
    if ($hash -ne $entry.sourceArchiveSha256.ToLowerInvariant()) {
        throw "Corresponding-source checksum mismatch for $fileName"
    }
    Write-Output "$fileName $hash"
}

Copy-Item -LiteralPath (Join-Path $projectRoot "THIRD-PARTY-SOURCES.md") -Destination $outputRoot -Force
Write-Output "Corresponding source archives fetched to $outputRoot"
