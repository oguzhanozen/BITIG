param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath
)

$ErrorActionPreference = "Stop"

$resolvedFile = (Resolve-Path -LiteralPath $FilePath).Path
$certificatePath = $env:WINDOWS_CERTIFICATE_PATH
$certificatePassword = $env:WINDOWS_CERTIFICATE_PASSWORD
if ([string]::IsNullOrWhiteSpace($certificatePath) -or
    -not (Test-Path -LiteralPath $certificatePath -PathType Leaf)) {
    throw "WINDOWS_CERTIFICATE_PATH must point to a code-signing PFX file"
}
if ([string]::IsNullOrWhiteSpace($certificatePassword)) {
    throw "WINDOWS_CERTIFICATE_PASSWORD is required"
}

$signTool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if (-not $signTool) {
    $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $signTool = Get-ChildItem -LiteralPath $kitsRoot -Filter signtool.exe -Recurse -File |
        Where-Object { $_.FullName -like "*\x64\signtool.exe" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
}
if (-not $signTool) {
    throw "signtool.exe was not found"
}
$signToolPath = if ($signTool.Path) { $signTool.Path } else { $signTool.FullName }

& $signToolPath sign /fd SHA256 /f $certificatePath /p $certificatePassword /tr https://timestamp.digicert.com /td SHA256 $resolvedFile
if ($LASTEXITCODE -ne 0) {
    throw "signtool failed for $resolvedFile"
}
& $signToolPath verify /pa /v $resolvedFile
if ($LASTEXITCODE -ne 0) {
    throw "signature verification failed for $resolvedFile"
}
