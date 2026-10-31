$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$temporaryBase = [IO.Path]::GetTempPath().TrimEnd([IO.Path]::DirectorySeparatorChar)
$fixtureRoot = Join-Path $temporaryBase "bitig-release-test-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path (Join-Path $fixtureRoot "scripts") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $fixtureRoot "src-tauri/binaries") -Force | Out-Null

function Assert-Verification {
    param([string[]]$Arguments, [bool]$Succeeds, [string]$Message)
    $ErrorActionPreference = "Continue"
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $fixtureRoot "scripts/verify-sidecars.ps1") -TargetTriple x86_64-pc-windows-msvc @Arguments 2>&1 | Out-String
    $ErrorActionPreference = "Stop"
    if (($LASTEXITCODE -eq 0) -ne $Succeeds -or $output -notmatch [regex]::Escape($Message)) {
        throw "Unexpected verifier result ($LASTEXITCODE): $output"
    }
}

try {
    foreach ($file in @("verify-sidecars.ps1", "sha256.ps1")) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $fixtureRoot "scripts/$file")
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot "licenses") -Destination $fixtureRoot -Recurse
    foreach ($file in @("LICENSE", "THIRD-PARTY-SOURCES.md")) {
        Copy-Item -LiteralPath (Join-Path $projectRoot $file) -Destination $fixtureRoot
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot "src-tauri/binaries/manifest.json") -Destination (Join-Path $fixtureRoot "src-tauri/binaries/manifest.json")

    Assert-Verification -Arguments @("-SkipBinaries") -Succeeds $true -Message "Sidecars verified"
    Assert-Verification -Arguments @() -Succeeds $false -Message "Missing release sidecar"
    Assert-Verification -Arguments @("-SkipBinaries", "-ReleaseGate") -Succeeds $false -Message "Unresolved RELEASE BLOCKER"
    $termsPath = Join-Path $fixtureRoot "licenses/INSTALLER-TERMS.txt"
    $fixtureTerms = Get-Content -LiteralPath $termsPath -Raw -Encoding UTF8
    $fixtureTerms.Replace("[RELEASE BLOCKER", "[fixture distributor") | Set-Content -LiteralPath $termsPath -Encoding UTF8
    Assert-Verification -Arguments @("-SkipBinaries", "-ReleaseGate") -Succeeds $true -Message "Sidecars verified"
    $manifestPath = Join-Path $fixtureRoot "src-tauri/binaries/manifest.json"
    $manifestText = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8
    $badManifest = $manifestText | ConvertFrom-Json
    $badManifest.schemaVersion = 99
    $badManifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Assert-Verification -Arguments @("-SkipBinaries") -Succeeds $false -Message "Unsupported or empty sidecar manifest"
    $manifestText | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Copy-Item -LiteralPath (Join-Path $projectRoot "src-tauri/tauri.release.conf.json") -Destination (Join-Path $fixtureRoot "src-tauri/tauri.release.conf.json")
    $configuration = Get-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri/tauri.release.conf.json") -Raw | ConvertFrom-Json
    $configuration.bundle.externalBin = @("binaries/ffmpeg")
    $configuration | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri/tauri.release.conf.json") -Encoding UTF8
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage") -Succeeds $false -Message "Online Installer must not bundle external binaries"

    $configuration.bundle.externalBin = @()
    $configuration.bundle | Add-Member -NotePropertyName licenseFile -NotePropertyValue "../licenses/INSTALLER-TERMS.txt" -Force
    $configuration.bundle | Add-Member -NotePropertyName windows -NotePropertyValue @{ nsis = @{ installMode = "currentUser"; template = "nsis/installer.nsi"; installerHooks = "nsis/installer-hooks.nsh" } } -Force
    $configuration | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri/tauri.release.conf.json") -Encoding UTF8
    $compiler = Join-Path $env:LOCALAPPDATA "tauri/NSIS/makensis.exe"
    if (-not (Test-Path -LiteralPath $compiler)) { throw "NSIS compiler is required for the package regression test; build with Tauri first." }
    $sevenZip = $env:BITIG_7ZIP_PATH
    if (-not $sevenZip) {
        $sevenZip = Join-Path $projectRoot "node_modules/.tmp/t23/7zip/portable/7z.exe"
        if (-not (Test-Path -LiteralPath $sevenZip)) { $sevenZip = (Get-Command 7z.exe -ErrorAction Stop).Source }
    }
    $badPackage = Join-Path $fixtureRoot "bad-setup.exe"
    $badScript = Join-Path $fixtureRoot "bad-installer.nsi"
    $fixtureLicense = Join-Path $fixtureRoot "LICENSE"
    @"
Unicode true
RequestExecutionLevel user
OutFile "$badPackage"
Section
SetOutPath `"`$INSTDIR`"
File /oname=ffmpeg.exe "$fixtureLicense"
SectionEnd
"@ | Set-Content -LiteralPath $badScript -Encoding UTF8
    & $compiler -V2 $badScript
    if ($LASTEXITCODE -ne 0) { throw "Could not build isolated package fixture" }
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage", "-InstallerPath", $badPackage, "-ScriptPath", $badScript, "-SevenZipPath", $sevenZip) -Succeeds $false -Message "Package contains media executable"
    $minimalScript = (Get-Content -LiteralPath $badScript -Raw -Encoding UTF8).Replace('File /oname=ffmpeg.exe', 'File /oname=$PLUGINSDIR\bitig-bootstrap.exe')
    $minimalScript = $minimalScript.Replace('SectionEnd', ('File /oname=bitig.exe "' + $fixtureLicense + '"' + "`r`nSectionEnd"))
    $minimalScript | Set-Content -LiteralPath $badScript -Encoding UTF8
    & $compiler -V2 $badScript
    if ($LASTEXITCODE -ne 0) { throw "Could not rebuild isolated package fixture" }
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage", "-InstallerPath", $badPackage, "-ScriptPath", $badScript, "-SevenZipPath", $sevenZip) -Succeeds $false -Message "Missing or duplicate package file: licenses/INSTALLER-TERMS.txt"
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage", "-InstallerPath", $badPackage, "-ScriptPath", $badScript, "-SevenZipPath", (Join-Path $fixtureRoot "missing-7z.exe")) -Succeeds $false -Message "7-Zip is required"

    # Recompile the generated installer with inert payload bytes; never execute it.
    $metadata = & cargo metadata --no-deps --locked --offline --format-version 1 --manifest-path (Join-Path $projectRoot "src-tauri/Cargo.toml") | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Could not locate generated installer" }
    $generatedRoot = Join-Path $metadata.target_directory "release/nsis/x64"
    $generatedScript = Get-Content -LiteralPath (Join-Path $generatedRoot "installer.nsi") -Raw -Encoding UTF8
    $fixtureHooksRoot = Join-Path $fixtureRoot "src-tauri/nsis"
    New-Item -ItemType Directory -Path $fixtureHooksRoot | Out-Null
    $sourceHooks = Join-Path $projectRoot "src-tauri/nsis/installer-hooks.nsh"
    $fixtureHooks = Join-Path $fixtureHooksRoot "installer-hooks.nsh"
    Copy-Item -LiteralPath $sourceHooks -Destination $fixtureHooks
    Copy-Item -LiteralPath (Join-Path $projectRoot "licenses/INSTALLER-TERMS.txt") -Destination $termsPath -Force
    $payload = Join-Path $fixtureRoot "bitig.exe"
    [IO.File]::WriteAllBytes($payload, [Text.Encoding]::ASCII.GetBytes('before __TAURI_BUNDLE_TYPE_VAR_NSS after'))
    $goodPackage = Join-Path $fixtureRoot "good-setup.exe"
    $goodScript = Join-Path $fixtureRoot "good-installer.nsi"
    $generatedScript = [regex]::Replace($generatedScript, '(?m)^!define MAINBINARYSRCPATH .+$', ('!define MAINBINARYSRCPATH "' + $payload + '"'))
    $generatedScript = [regex]::Replace($generatedScript, '(?m)^!define OUTFILE .+$', ('!define OUTFILE "' + $goodPackage + '"'))
    $generatedScript.Replace($sourceHooks, $fixtureHooks) | Set-Content -LiteralPath $goodScript -Encoding UTF8
    Push-Location $generatedRoot
    try { & $compiler /NOCD -V2 $goodScript } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "Could not compile isolated generated installer" }
    [IO.File]::WriteAllBytes($payload, [Text.Encoding]::ASCII.GetBytes('before __TAURI_BUNDLE_TYPE_VAR_UNK after'))
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage", "-InstallerPath", $goodPackage, "-ScriptPath", $goodScript, "-SevenZipPath", $sevenZip) -Succeeds $true -Message "Online Installer package verified"
    [IO.File]::WriteAllBytes($payload, [Text.Encoding]::ASCII.GetBytes('Before __TAURI_BUNDLE_TYPE_VAR_UNK after'))
    Assert-Verification -Arguments @("-SkipBinaries", "-VerifyInstallerPackage", "-InstallerPath", $goodPackage, "-ScriptPath", $goodScript, "-SevenZipPath", $sevenZip) -Succeeds $false -Message "Packaged executable does not match the new build"
    Write-Output "Release verification tests passed."
}
finally {
    $resolvedFixture = (Resolve-Path -LiteralPath $fixtureRoot).Path
    if (-not $resolvedFixture.StartsWith($temporaryBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Test cleanup target escaped the temporary directory."
    }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
}
