param(
    [Parameter(Mandatory = $true)]
    [string]$ArtifactDirectory,
    [string]$OutputFile = "SHA256SUMS.txt"
)

$ErrorActionPreference = "Stop"
$artifactRoot = Resolve-Path -LiteralPath $ArtifactDirectory
$artifactRootPath = $artifactRoot.Path.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
)
$outputPath = Join-Path $artifactRoot $OutputFile
$lines = Get-ChildItem -LiteralPath $artifactRoot -File -Recurse |
    Where-Object { $_.Name -ne $OutputFile } |
    Sort-Object FullName |
    ForEach-Object {
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $relative = $_.FullName.Substring($artifactRootPath.Length).TrimStart(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        ).Replace("\", "/")
        "$hash  $relative"
    }
if (-not $lines) {
    throw "No release artifacts found in $artifactRoot"
}
[System.IO.File]::WriteAllLines(
    $outputPath,
    [string[]]$lines,
    [System.Text.UTF8Encoding]::new($false)
)
Get-Content -LiteralPath $outputPath
