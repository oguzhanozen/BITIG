param(
    [string]$NsisDirectory = "$env:LOCALAPPDATA\tauri\NSIS",
    [string]$InstallerPath = ""
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$webviewInstallMode = (Get-Content -LiteralPath (Join-Path $projectRoot 'src-tauri/tauri.release.conf.json') -Raw | ConvertFrom-Json).bundle.windows.webviewInstallMode
if ($webviewInstallMode.type -ne 'embedBootstrapper' -or $webviewInstallMode.silent -ne $true) {
    throw 'Release installers must embed the WebView2 bootstrapper and run it silently.'
}
Write-Output 'NSIS WebView2 configuration passed: embedded bootstrapper, silent installation.'
$testRoot = Join-Path $projectRoot 'node_modules\.tmp\nsis-tests'
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
$hooks = Join-Path $projectRoot 'src-tauri\nsis\installer-hooks.nsh'
$fixture = @'
Unicode true
RequestExecutionLevel user
!include LogicLib.nsh
!include FileFunc.nsh
!include "HOOKS"
OutFile "OUTPUT"
!insertmacro BITIG_COMMAND_LINE_FUNCTION
Function .onInit
  Call BitigRejectUnattended
  SetErrorLevel 0
  Quit
FunctionEnd
Section
SectionEnd
'@
$script = Join-Path $testRoot 'command-line.nsi'
$executable = Join-Path $testRoot 'command-line.exe'
[IO.File]::WriteAllText($script, $fixture.Replace('HOOKS', $hooks).Replace('OUTPUT', $executable))
& (Join-Path $NsisDirectory 'makensis.exe') /V2 $script
if ($LASTEXITCODE -ne 0) { throw 'NSIS command-line fixture did not compile.' }

# This fixture has no installation, registry, or application-data instructions.
foreach ($case in @(
    @{ Arguments = '/S'; Expected = 2 },
    @{ Arguments = '/P'; Expected = 2 },
    @{ Arguments = '/s'; Expected = 2 },
    @{ Arguments = '/p'; Expected = 2 },
    @{ Arguments = '/NS /S'; Expected = 2 },
    @{ Arguments = '/R /p'; Expected = 2 },
    @{ Arguments = ''; Expected = 0 },
    @{ Arguments = '/NS /R'; Expected = 0 },
    @{ Arguments = '/PROJECT'; Expected = 0 },
    @{ Arguments = '/ARGS="/P"'; Expected = 0 },
    @{ Arguments = '/D=C:\Some /P directory'; Expected = 0 }
)) {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $executable
    $start.Arguments = $case.Arguments
    $start.UseShellExecute = $false
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'NSIS fixture timed out.' }
    if ($process.ExitCode -ne $case.Expected) {
        throw "Unexpected exit $($process.ExitCode) for '$($case.Arguments)' (expected $($case.Expected))."
    }
}
Write-Output 'NSIS command-line behavior passed (11 cases; no actual setup executed).'

# Exercise the real MUI page and BITIG page callbacks without any Install action.
$uiFixture = @'
Unicode true
ManifestDPIAware true
ManifestDPIAwareness PerMonitorV2
RequestExecutionLevel user
!include MUI2.nsh
!include FileFunc.nsh
!include "HOOKS"
!define LICENSE "TERMS"
!define MAINBINARYSRCPATH "STUB"
!define MAINBINARYNAME "bitig-fixture"
!define MANUPRODUCTKEY "Software\BITIG-NSIS-Fixture"
!define UNINSTKEY "Software\BITIG-NSIS-Fixture\Uninstall"
Var PassiveMode
Var WixMode
Name "BITIG NSIS UI Fixture"
OutFile "OUTPUT"
InstallDir "DESTINATION"
!insertmacro BITIG_LICENSE_PAGE
!insertmacro MUI_PAGE_DIRECTORY
Page custom BitigMediaToolsPage BitigMediaToolsLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro BITIG_FUNCTIONS
Function .onInit
  !insertmacro BITIG_INIT
FunctionEnd
Section
  SetErrorLevel 0
SectionEnd
'@
$uiExecutable = Join-Path $testRoot 'ui-fixture.exe'
$uiScript = Join-Path $testRoot 'ui-fixture.nsi'
$uiFixture = $uiFixture.Replace('HOOKS', $hooks).Replace('TERMS', (Join-Path $projectRoot 'licenses\INSTALLER-TERMS.txt')).Replace('STUB', $executable).Replace('OUTPUT', $uiExecutable).Replace('DESTINATION', (Join-Path $testRoot 'unused-destination'))
[IO.File]::WriteAllText($uiScript, $uiFixture)
& (Join-Path $NsisDirectory 'makensis.exe') /V2 $uiScript
if ($LASTEXITCODE -ne 0) { throw 'NSIS UI fixture did not compile.' }

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class NsisFixtureWindow {
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] static extern IntPtr GetText(IntPtr window, uint message, IntPtr length, StringBuilder text);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr window);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] static extern int MapWindowPoints(IntPtr from, IntPtr to, ref Rect rect, uint count);
    public static IntPtr ForProcess(int process) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((w,d) => { uint p; GetWindowThreadProcessId(w,out p); if(p == process && GetDlgItem(w,1) != IntPtr.Zero) result=w; return true; },IntPtr.Zero);
        return result;
    }
    public static IntPtr Text(IntPtr parent, string prefix) {
        IntPtr result = IntPtr.Zero;
        EnumChildWindows(parent,(w,d) => { var s=new StringBuilder(1024); GetWindowText(w,s,s.Capacity); if(s.ToString().StartsWith(prefix)) result=w; return true; },IntPtr.Zero);
        return result;
    }
    public static void Click(IntPtr control) { PostMessage(control,0xF5,IntPtr.Zero,IntPtr.Zero); System.Threading.Thread.Sleep(100); }
    public static bool Checked(IntPtr control) { return SendMessage(control,0xF0,IntPtr.Zero,IntPtr.Zero).ToInt32()==1; }
    public static string Content(IntPtr control) { var s=new StringBuilder(SendMessage(control,0xE,IntPtr.Zero,IntPtr.Zero).ToInt32()+1); GetText(control,0xD,new IntPtr(s.Capacity),s); return s.ToString(); }
    public static IntPtr Sibling(IntPtr control,int id) { return GetDlgItem(GetParent(control),id); }
    public static bool Fits(IntPtr control) {
        Rect c,p; var parent=GetParent(control); GetWindowRect(control,out c); GetClientRect(parent,out p); MapWindowPoints(parent,IntPtr.Zero,ref p,2);
        return c.Left>=p.Left && c.Top>=p.Top && c.Right<=p.Right && c.Bottom<=p.Bottom;
    }
    public static string Bounds(IntPtr control) {
        Rect c,p; var parent=GetParent(control); GetWindowRect(control,out c); GetClientRect(parent,out p); MapWindowPoints(parent,IntPtr.Zero,ref p,2);
        return string.Format("control ({0},{1})-({2},{3}), page ({4},{5})-({6},{7})",c.Left,c.Top,c.Right,c.Bottom,p.Left,p.Top,p.Right,p.Bottom);
    }
}
'@
function Wait-Control([IntPtr]$Window, [string]$Prefix) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $control = [NsisFixtureWindow]::Text($Window, $Prefix)
        if ($control -ne [IntPtr]::Zero) { return $control }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "NSIS fixture control did not appear: $Prefix"
}
function Click-PageButton([IntPtr]$Window, [int]$Id) {
    [NsisFixtureWindow]::Click([NsisFixtureWindow]::GetDlgItem($Window, $Id))
    Start-Sleep -Milliseconds 150
}
$start = New-Object Diagnostics.ProcessStartInfo
$start.FileName = $uiExecutable
$start.UseShellExecute = $false
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$uiProcess = [Diagnostics.Process]::Start($start)
try {
    $window = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 100 -and $window -eq [IntPtr]::Zero; $attempt++) {
        Start-Sleep -Milliseconds 50
        $window = [NsisFixtureWindow]::ForProcess($uiProcess.Id)
    }
    if ($window -eq [IntPtr]::Zero) { throw 'NSIS fixture window did not appear.' }
    $accept = Wait-Control $window 'I accept the BITIG Terms of Use'
    $licenseText = [NsisFixtureWindow]::Content([NsisFixtureWindow]::Sibling($accept, 1000))
    foreach ($part in @('PART I - BITIG Terms of Use', 'PART II - Third-Party Software and Licensing Information', 'PART III - Local Data Storage and Network Connections')) {
        if (-not $licenseText.Contains($part)) { throw "The license control is missing $part" }
    }
    $next = [NsisFixtureWindow]::GetDlgItem($window, 1)
    if ([NsisFixtureWindow]::Checked($accept) -or [NsisFixtureWindow]::IsWindowEnabled($next)) { throw 'Terms must initially be unchecked and block Next.' }
    [NsisFixtureWindow]::Click($accept)
    if (-not [NsisFixtureWindow]::IsWindowEnabled($next)) { throw 'Accepting terms must enable Next.' }
    [NsisFixtureWindow]::Click($accept)
    if ([NsisFixtureWindow]::IsWindowEnabled($next)) { throw 'Removing acceptance must disable Next.' }
    [NsisFixtureWindow]::Click($accept)
    Click-PageButton $window 1
    Click-PageButton $window 1
    $yt = Wait-Control $window ('yt-dlp ' + [char]0x2014 + ' Video and audio downloading')
    $ff = Wait-Control $window ('FFmpeg + ffprobe ' + [char]0x2014 + ' Media processing and analysis')
    $null = Wait-Control $window 'BITIG requires yt-dlp, FFmpeg, and ffprobe'
    $null = Wait-Control $window 'Required components'
    foreach ($required in @($yt, $ff)) {
        if (-not [NsisFixtureWindow]::Checked($required) -or [NsisFixtureWindow]::IsWindowEnabled($required)) { throw 'Required media tools must be selected and locked.' }
    }
    $auto = Wait-Control $window 'Automatically update media tools'
    foreach ($control in @($yt, $ff, $auto)) {
        if (-not [NsisFixtureWindow]::Fits($control)) { throw ('Media tool control is clipped outside the page: ' + [NsisFixtureWindow]::Bounds($control)) }
    }
    if ([NsisFixtureWindow]::Checked($auto) -or -not [NsisFixtureWindow]::IsWindowEnabled($auto)) { throw 'Automatic updates must be optional and initially off.' }
    [NsisFixtureWindow]::Click($auto)
    Click-PageButton $window 3
    Click-PageButton $window 3
    $accept = Wait-Control $window 'I accept the BITIG Terms of Use'
    [NsisFixtureWindow]::Click($accept)
    if ([NsisFixtureWindow]::IsWindowEnabled($next)) { throw 'Back navigation must retain a coherent acceptance state.' }
    [NsisFixtureWindow]::Click($accept)
    Click-PageButton $window 1
    Click-PageButton $window 1
    $auto = Wait-Control $window 'Automatically update media tools'
    if (-not [NsisFixtureWindow]::Checked($auto)) { throw 'Update choice was lost during Back/Next navigation.' }
    [NsisFixtureWindow]::Click($auto)
    if ([NsisFixtureWindow]::Checked($auto)) { throw 'Automatic updates cannot be disabled.' }
    if (Test-Path -LiteralPath (Join-Path $testRoot 'unused-destination')) { throw 'UI fixture unexpectedly created an installation directory.' }
    Write-Output 'NSIS UI behavior passed: native acceptance, ID 1034 validation, locked tools, optional updates and Back/Next state (no actual setup executed).'
}
finally {
    if (-not $uiProcess.HasExited) { $uiProcess.Kill(); $uiProcess.WaitForExit() }
    $uiProcess.Dispose()
}

if ($InstallerPath) {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
        throw 'Actual setup execution is restricted to an ephemeral GitHub-hosted runner.'
    }
    $InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
    $protectedLocations = @(
        (Join-Path $env:LOCALAPPDATA 'BITIG'),
        (Join-Path $env:LOCALAPPDATA 'app.bitig.desktop'),
        'HKCU:\Software\bitig\BITIG',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\BITIG'
    )
    foreach ($location in $protectedLocations) {
        if (Test-Path -LiteralPath $location) { throw 'Actual setup rejection test requires a fresh ephemeral profile.' }
    }
    foreach ($arguments in @('/S', '/P', '/s', '/p', '/NS /S', '/R /p')) {
        $start.FileName = $InstallerPath
        $start.Arguments = $arguments
        $process = [Diagnostics.Process]::Start($start)
        try {
            if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'Actual setup did not reject unattended mode promptly.' }
            if ($process.ExitCode -ne 2) { throw "Actual setup returned $($process.ExitCode) for $arguments, expected 2." }
            foreach ($location in $protectedLocations) {
                if (Test-Path -LiteralPath $location) { throw "Rejected setup changed $location" }
            }
        }
        finally { $process.Dispose() }
    }
    Write-Output 'Actual setup rejected all unattended modes on the ephemeral runner.'
}
