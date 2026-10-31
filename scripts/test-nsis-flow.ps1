param(
    [string]$NsisDirectory = "$env:LOCALAPPDATA\tauri\NSIS",
    [string]$InstallerPath = ""
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'test-nsis.ps1') -NsisDirectory $NsisDirectory -InstallerPath $InstallerPath

$stub = Join-Path $testRoot ("flow-bootstrap-" + [Guid]::NewGuid().ToString('N') + '.exe')
$stubSource = @'
using System;
using System.IO;
public static class NsisFlowBootstrap {
    public static int Main(string[] args) {
        string root = Environment.GetEnvironmentVariable("BITIG_NSIS_FLOW_ROOT");
        string allowed = Path.GetFullPath(@"TESTROOT") + Path.DirectorySeparatorChar;
        if (String.IsNullOrEmpty(root) || !Path.GetFullPath(root).StartsWith(allowed, StringComparison.OrdinalIgnoreCase)) return 2;
        File.AppendAllText(Path.Combine(root,"events.log"),String.Join(" ",args)+Environment.NewLine);
        if (args.Length == 1 && args[0] == "--old-uninstall") return 0;
        if (args.Length == 2 && args[0] == "--installer-commit-preferences")
            return Environment.GetEnvironmentVariable("BITIG_NSIS_FLOW_RESULT") == "commit-fail" ? 1 : 0;
        if (args.Length != 3 || args[0] != "--installer-bootstrap" || args[2] != "--defer-preferences") return 2;
        Console.WriteLine("Checking existing BITIG tools...");
        string attempt = Path.Combine(root,"attempted");
        bool first = !File.Exists(attempt);
        File.WriteAllText(attempt,"attempted");
        string result = Environment.GetEnvironmentVariable("BITIG_NSIS_FLOW_RESULT");
        if (result == "fail" || (result == "retry" && first)) return 1;
        Console.WriteLine("Downloading yt-dlp...");
        Console.WriteLine("Downloading FFmpeg...");
        Console.WriteLine("Verifying SHA-256...");
        Console.WriteLine("Installing verified media tools...");
        Console.WriteLine("Required media tools are ready.");
        return 0;
    }
}
'@
Add-Type -TypeDefinition $stubSource.Replace('TESTROOT', $testRoot.Replace('"', '""')) -OutputAssembly $stub -OutputType WindowsApplication

$flowExecutable = Join-Path $testRoot 'flow-fixture.exe'
$flowScript = Join-Path $testRoot 'flow-fixture.nsi'
$flowFixture = $uiFixture.Replace("`r`n", "`n")
$flowFixture = $flowFixture.Replace('!define MAINBINARYSRCPATH "' + $executable + '"', '!define MAINBINARYSRCPATH "' + $stub + '"')
$flowFixture = $flowFixture.Replace('OutFile "' + $uiExecutable + '"', 'OutFile "' + $flowExecutable + '"')
$flowFixture = $flowFixture.Replace('Name "BITIG NSIS UI Fixture"', 'Name "BITIG NSIS Flow Fixture"')
$flowFixture = $flowFixture.Replace('!insertmacro MUI_PAGE_INSTFILES', @'
!define MUI_FINISHPAGE_NOAUTOCLOSE
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "BITIG fixture completed successfully"
!insertmacro MUI_PAGE_FINISH
'@)
$flowFixture = $flowFixture.Replace('  !insertmacro BITIG_INIT', @'
  !insertmacro BITIG_INIT
  ReadEnvStr $BitigPreviousDirectory "BITIG_NSIS_FLOW_ROOT"
  StrCpy $BitigPreviousUninstaller '"STUB" --old-uninstall'
'@.Replace('STUB', $stub))
$emptySection = "Section`n  SetErrorLevel 0`nSectionEnd"
if (-not $flowFixture.Contains($emptySection)) { throw 'The inert NSIS fixture section changed; flow tests must be updated.' }
$flowFixture = $flowFixture.Replace($emptySection, @'
Section
  Call BitigProvisionTools
  Call BitigRunPreviousUninstaller
  ClearErrors
  FileOpen $0 "$BitigPreviousDirectory\installed.marker" w
  FileWrite $0 "fixture files installed"
  FileClose $0
  FileOpen $0 "$BitigPreviousDirectory\events.log" a
  FileSeek $0 0 END
  FileWrite $0 "installed-files$\r$\n"
  FileClose $0
  !insertmacro NSIS_HOOK_POSTINSTALL
  SetErrorLevel 0
SectionEnd
'@)
[IO.File]::WriteAllText($flowScript, $flowFixture)
& (Join-Path $NsisDirectory 'makensis.exe') /V2 $flowScript
if ($LASTEXITCODE -ne 0) { throw 'NSIS flow fixture did not compile.' }

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class NsisFlowDialog {
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr window, Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder name, int size);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    public static void ClickButton(IntPtr window, int id) { PostMessage(window,0x111,new IntPtr(id),GetDlgItem(window,id)); }
    public static int ProgressRows(IntPtr window) {
        int result=-1;
        EnumChildWindows(window,(w,d) => {
            var name=new StringBuilder(128); GetClassName(w,name,name.Capacity);
            if(name.ToString()=="SysListView32") result=SendMessage(w,0x1004,IntPtr.Zero,IntPtr.Zero).ToInt32();
            return true;
        },IntPtr.Zero);
        return result;
    }
    public static IntPtr RetryPrompt(int process) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((w,d) => { uint p; GetWindowThreadProcessId(w,out p); if(p == process && GetDlgItem(w,4) != IntPtr.Zero && GetDlgItem(w,2) != IntPtr.Zero) result=w; return true; },IntPtr.Zero);
        return result;
    }
}
'@
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Wait-RetryPrompt([Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $prompt = [NsisFlowDialog]::RetryPrompt($Process.Id)
        if ($prompt -ne [IntPtr]::Zero) { return $prompt }
        if ($Process.HasExited) { throw 'The flow fixture exited before offering Retry/Cancel.' }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    $window = [NsisFixtureWindow]::ForProcess($Process.Id)
    $element = [Windows.Automation.AutomationElement]::FromHandle($window)
    $controls = $element.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
    $state = @($controls | ForEach-Object { $_.Current.Name } | Select-Object -First 20) -join '; '
    throw "The flow fixture did not offer Retry/Cancel. Current controls: $state"
}

# Run the current template's Install section, not a hand-written registry simulation.
$template = [IO.File]::ReadAllText((Join-Path $projectRoot 'src-tauri/nsis/installer.nsi'))
$installSection = [regex]::Match($template, '(?ms)^Section Install\r?\n.*?^SectionEnd').Value
if (-not $installSection) { throw 'The production Install section was not found.' }
# These optional payload/association loops are empty in this isolated fixture.
while ($installSection.Contains('{{#each')) {
    $rendered = [regex]::Replace($installSection, '(?s){{#each(?:(?!{{#each).)*?{{/each}}', '')
    if ($rendered -eq $installSection) { throw 'Could not isolate the template loops.' }
    $installSection = $rendered
}
if ($installSection.Contains('{{')) { throw 'Unrendered Install section template expression.' }
$shortcutFunctions = [regex]::Matches($template, '(?ms)^Function CreateOrUpdate(?:StartMenu|Desktop)Shortcut\r?\n.*?^FunctionEnd').Value -join "`n"
if (-not $shortcutFunctions) { throw 'The production shortcut functions were not found.' }
$shortcutFunctions = $shortcutFunctions.Replace('$SMPROGRAMS', '$INSTDIR\start-menu').Replace('$DESKTOP', '$INSTDIR\desktop')
$generatedRoot = Join-Path $projectRoot 'src-tauri/target/release/nsis/x64'
$registryRoot = 'Software\BITIG-NSIS-Fixture-' + [Guid]::NewGuid().ToString('N')
$installFixture = $uiFixture.Replace("`r`n", "`n").Replace('Software\BITIG-NSIS-Fixture', $registryRoot)
$installFixture = $installFixture.Replace('!define MAINBINARYSRCPATH "' + $executable + '"', '!define MAINBINARYSRCPATH "' + $stub + '"')
$installFixture = $installFixture.Replace('!define MAINBINARYNAME "bitig-fixture"', '!define MAINBINARYNAME "' + [IO.Path]::GetFileNameWithoutExtension($stub) + '"')
$installFixture = $installFixture.Replace('!include MUI2.nsh', @'
!include MUI2.nsh
!include x64.nsh
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Win\RestartManager.nsh"
!include "UTILS"
!addplugindir "PLUGINS"
!define INSTALLMODE "currentUser"
!define ARCH "x64"
!define PRODUCTNAME "BITIG NSIS Fixture"
!define MANUFACTURER "Fixture"
!define VERSION "0.1.5"
!define ESTIMATEDSIZE 1
!define HOMEPAGE "https://example.invalid/"
!define STARTMENUFOLDER ""
!define BUNDLEID "bitig.nsis.fixture"
Var OldMainBinaryName
Var UpdateMode
Var NoShortcutMode
Var AppStartMenuFolder
Var MultiUser.InstallMode
LangString appRunning 1033 "Fixture is running"
LangString appRunningOkKill 1033 "Close fixture"
LangString failedToKillApp 1033 "Could not close fixture"
!macro FixtureTrace stage
  StrCpy $9 0
  IfErrors 0 +2
    StrCpy $9 1
  FileOpen $8 "$BitigPreviousDirectory\flags.log" a
  FileSeek $8 0 END
  FileWrite $8 "${stage}=$9$\r$\n"
  FileClose $8
  ${If} $9 = 1
    SetErrors
  ${Else}
    ClearErrors
  ${EndIf}
!macroend
'@.Replace('UTILS', (Join-Path $generatedRoot 'utils.nsh')).Replace('PLUGINS', (Join-Path $NsisDirectory 'Plugins/x86-unicode/additional')))
$installFixture = $installFixture.Replace('!insertmacro MUI_PAGE_DIRECTORY', "!insertmacro MUI_PAGE_DIRECTORY`n!insertmacro MUI_PAGE_STARTMENU Application `$AppStartMenuFolder")
# Only this inert fixture bypasses interactive acceptance so it can run with /S.
$installFixture = $installFixture.Replace('  !insertmacro BITIG_INIT', @'
  ReadEnvStr $BitigPreviousDirectory "BITIG_NSIS_FLOW_ROOT"
  StrCpy $INSTDIR "$BitigPreviousDirectory\installed"
  CreateDirectory "$INSTDIR\start-menu"
  CreateDirectory "$INSTDIR\desktop"
  !insertmacro SetContext
  StrCpy $BitigPreviousUninstaller ""
  StrCpy $BitigTermsAccepted 1
  StrCpy $BitigAutoUpdate "disabled"
  StrCpy $PassiveMode 0
  StrCpy $MultiUser.InstallMode "CurrentUser"
  SetOutPath "$BitigPreviousDirectory\fallback"
'@)
$readOldName = '  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"'
$installSection = $installSection.Replace($readOldName, $readOldName + "`n  !insertmacro FixtureTrace optional-read")
$noRepair = '  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"'
$installSection = $installSection.Replace($noRepair, $noRepair + "`n  !insertmacro FixtureTrace registration")
$emptySection = "Section`n  SetErrorLevel 0`nSectionEnd"
if (-not $installFixture.Contains($emptySection)) { throw 'The inert fixture section changed.' }
$installFixture = $installFixture.Replace($emptySection, "Section BitigPrepare`n  Call BitigProvisionTools`nSectionEnd`n" + $installSection + "`n" + $shortcutFunctions + "`nSection Uninstall`nSectionEnd")
$installScript = Join-Path $testRoot 'install-errors.nsi'
$installExecutable = Join-Path $testRoot 'install-errors.exe'
$installFixture = $installFixture.Replace('OutFile "' + $uiExecutable + '"', 'OutFile "' + $installExecutable + '"')
try {
    foreach ($case in @(
        @{ Name = 'first-install'; Expected = 0 },
        @{ Name = 'first-install-both'; Mode = 'both'; Expected = 0 },
        @{ Name = 'renamed-binary'; OldName = 'old-binary.exe'; Expected = 0 },
        @{ Name = 'unchanged-binary'; OldName = [IO.Path]::GetFileName($stub); Expected = 0 },
        @{ Name = 'out-path-failure'; OutPathFails = $true; Expected = 1 },
        @{ Name = 'manufacturer-write-failure'; Fault = '  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR'; Expected = 1 },
        @{ Name = 'install-mode-write-failure'; Mode = 'both'; Fault = '    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1'; Expected = 1 },
        @{ Name = 'binary-name-write-failure'; Fault = '  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"'; Expected = 1 },
        @{ Name = 'uninstall-dword-failure'; Fault = '  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"'; Expected = 1 },
        @{ Name = 'estimated-size-failure'; Fault = '  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"'; Expected = 1 },
        @{ Name = 'url-info-failure'; Fault = '    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"'; Expected = 1 },
        @{ Name = 'url-update-failure'; Fault = '    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"'; Expected = 1 },
        @{ Name = 'help-link-failure'; Fault = '    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"'; Expected = 1 },
        @{ Name = 'bootstrap-failure'; Result = 'fail'; Expected = 1 },
        @{ Name = 'preference-commit-failure'; Result = 'commit-fail'; Expected = 1 }
    )) {
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($registryRoot, $false)
        $flowRoot = Join-Path $testRoot ($case.Name + '-' + [Guid]::NewGuid().ToString('N'))
        $destination = Join-Path $flowRoot 'installed'
        New-Item -ItemType Directory -Path $destination | Out-Null
        $caseExecutable = Join-Path $flowRoot 'install-errors.exe'
        $caseFixture = $installFixture.Replace('OutFile "' + $installExecutable + '"', 'OutFile "' + $caseExecutable + '"')
        if ($case.Mode) { $caseFixture = $caseFixture.Replace('!define INSTALLMODE "currentUser"', '!define INSTALLMODE "' + $case.Mode + '"') }
        if ($case.OldName) {
            $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($registryRoot + '\Uninstall')
            try { $key.SetValue('MainBinaryName', $case.OldName) } finally { $key.Dispose() }
            [IO.File]::WriteAllText((Join-Path $destination $case.OldName), 'previous binary')
        }
        if ($case.OutPathFails) {
            [IO.File]::WriteAllText((Join-Path $destination 'blocker'), 'file blocks directory creation')
            $caseFixture = $caseFixture.Replace('  SetOutPath $INSTDIR', '  SetOutPath "$INSTDIR\blocker\child"')
        }
        if ($case.Fault) {
            if (-not $caseFixture.Contains($case.Fault)) { throw "Fault target disappeared: $($case.Name)" }
            $keyToken = [regex]::Match($case.Fault, '"\$\{(?:MANUPRODUCTKEY|UNINSTKEY)\}"').Value
            # An overlong key component fails inside our temporary registry namespace.
            $invalidKey = $keyToken.TrimEnd('"') + '\' + ('x' * 300) + '"'
            $caseFixture = $caseFixture.Replace($case.Fault, $case.Fault.Replace($keyToken, $invalidKey))
        }
        [IO.File]::WriteAllText($installScript, $caseFixture)
        & (Join-Path $NsisDirectory 'makensis.exe') /V2 $installScript
        if ($LASTEXITCODE -ne 0) { throw 'The production Install section fixture did not compile.' }
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = $caseExecutable
        $start.Arguments = '/S'
        $start.UseShellExecute = $false
        $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
        $start.EnvironmentVariables['BITIG_NSIS_FLOW_ROOT'] = $flowRoot
        $start.EnvironmentVariables['BITIG_NSIS_FLOW_RESULT'] = $case.Result
        $process = [Diagnostics.Process]::Start($start)
        try {
            if ($case.Result) {
                $prompt = Wait-RetryPrompt $process
                [NsisFlowDialog]::ClickButton($prompt, 2)
            }
            if (-not $process.WaitForExit(60000)) { $process.Kill(); $process.WaitForExit(); throw "Install section fixture timed out: $($case.Name)" }
            $flags = @()
            if (Test-Path -LiteralPath (Join-Path $flowRoot 'flags.log')) { $flags = @(Get-Content -LiteralPath (Join-Path $flowRoot 'flags.log')) }
            $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($registryRoot + '\Uninstall')
            try { $displayName = if ($key) { $key.GetValue('DisplayName') } } finally { if ($key) { $key.Dispose() } }
            if ($process.ExitCode -ne $case.Expected) {
                throw "$($case.Name): exit $($process.ExitCode), expected $($case.Expected); flags: $($flags -join ', '); registry=$registryRoot; DisplayName=$displayName"
            }
            $events = @(Get-Content -LiteralPath (Join-Path $flowRoot 'events.log'))
            $expected = @('--installer-bootstrap --auto-update=disabled --defer-preferences')
            if ($case.Expected -eq 0 -or $case.Result -eq 'commit-fail') { $expected += '--installer-commit-preferences --auto-update=disabled' }
            if ($case.Result -eq 'fail' -and (Test-Path -LiteralPath (Join-Path $destination ([IO.Path]::GetFileName($stub))))) { throw 'Bootstrap failure allowed application installation.' }
            if (($events -join "`n") -ne ($expected -join "`n")) { throw "Unexpected bootstrap/commit progression: $($case.Name)" }
            if ($case.Expected -eq 0) {
                $readFlag = if ($case.OldName) { 0 } else { 1 }
                if (($flags -join ',') -ne "optional-read=$readFlag,registration=0" -or $displayName -ne 'BITIG NSIS Fixture') { throw 'Optional read or registration failed.' }
                if (-not (Test-Path -LiteralPath (Join-Path $destination ([IO.Path]::GetFileName($stub))))) { throw 'The new application binary was not installed.' }
                if ($case.OldName -eq 'old-binary.exe' -and (Test-Path -LiteralPath (Join-Path $destination $case.OldName))) { throw 'Renamed previous binary was not removed.' }
            }
            Write-Output "NSIS Install section passed: $($case.Name) (exit $($process.ExitCode))."
        }
        finally {
            if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
            $process.Dispose()
        }
    }
}
finally {
    if ($registryRoot -notmatch '^Software\\BITIG-NSIS-Fixture-[a-f0-9]{32}$') { throw 'Unsafe fixture registry cleanup target.' }
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($registryRoot, $false)
}
Write-Output 'NSIS production Install section passed: 15 isolated executable cases; real hooks, process checks and redirected shortcut functions; optional payload/association loops omitted; no WebView2 or real BITIG setup.'

foreach ($case in @(
    @{ Name = 'cancel'; Result = 'fail'; Update = 'disabled' },
    @{ Name = 'retry'; Result = 'retry'; Update = 'enabled' }
)) {
    $flowRoot = Join-Path $testRoot ($case.Name + '-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $flowRoot | Out-Null
    $sentinel = Join-Path $flowRoot 'previous-application.marker'
    [IO.File]::WriteAllText($sentinel, 'previous application remains untouched')
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $flowExecutable
    $start.UseShellExecute = $false
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.EnvironmentVariables['BITIG_NSIS_FLOW_ROOT'] = $flowRoot
    $start.EnvironmentVariables['BITIG_NSIS_FLOW_RESULT'] = $case.Result
    $flowProcess = [Diagnostics.Process]::Start($start)
    try {
        $window = [IntPtr]::Zero
        for ($attempt = 0; $attempt -lt 100 -and $window -eq [IntPtr]::Zero; $attempt++) {
            Start-Sleep -Milliseconds 50
            $window = [NsisFixtureWindow]::ForProcess($flowProcess.Id)
        }
        if ($window -eq [IntPtr]::Zero) { throw 'The flow fixture window did not appear.' }
        [NsisFixtureWindow]::Click((Wait-Control $window 'I accept the BITIG Terms of Use'))
        Click-PageButton $window 1
        Click-PageButton $window 1
        $auto = Wait-Control $window 'Automatically update media tools'
        if ($case.Update -eq 'enabled') { [NsisFixtureWindow]::Click($auto) }
        $install = [NsisFixtureWindow]::GetDlgItem($window, 1)
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while (-not [NsisFixtureWindow]::IsWindowEnabled($install) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        if (-not [NsisFixtureWindow]::IsWindowEnabled($install)) { throw 'The fixture Install button remained disabled.' }
        Start-Sleep -Milliseconds 200
        [NsisFlowDialog]::ClickButton($window, 1)
        $prompt = Wait-RetryPrompt $flowProcess
        $failedRows = [NsisFlowDialog]::ProgressRows($window)
        if ($failedRows -lt 1) { throw 'The fixture did not expose the NSIS installation log.' }
        $expectedProvision = '--installer-bootstrap --auto-update=' + $case.Update + ' --defer-preferences'
        if ((Get-Content -LiteralPath (Join-Path $flowRoot 'events.log')).Count -ne 1) { throw 'Bootstrap failure allowed a later action to run.' }
        if (Test-Path -LiteralPath (Join-Path $flowRoot 'installed.marker')) { throw 'Files were installed before successful provisioning.' }
        if ([IO.File]::ReadAllText($sentinel) -ne 'previous application remains untouched') { throw 'Bootstrap failure modified the previous application.' }
        if ($case.Name -eq 'cancel') {
            [NsisFlowDialog]::ClickButton($prompt, 2)
            Start-Sleep -Milliseconds 300
            if ([NsisFixtureWindow]::Text($window, 'BITIG fixture completed successfully') -ne [IntPtr]::Zero) { throw 'Failed installation reached the success finish page.' }
            $closeId = 2
            if ([NsisFixtureWindow]::IsWindowEnabled([NsisFixtureWindow]::GetDlgItem($window, 1))) { $closeId = 1 }
            [NsisFlowDialog]::ClickButton($window, $closeId)
            if (-not $flowProcess.WaitForExit(5000)) { throw 'Cancelled installation did not exit.' }
            if ($flowProcess.ExitCode -eq 0) { throw 'Cancelled bootstrap was reported successful.' }
            $events = @(Get-Content -LiteralPath (Join-Path $flowRoot 'events.log'))
            if ($events.Count -ne 1 -or $events[0] -ne $expectedProvision) { throw 'Cancelled bootstrap ran uninstall, file installation, or preference commit.' }
        }
        else {
            [NsisFlowDialog]::ClickButton($prompt, 4)
            $deadline = [DateTime]::UtcNow.AddSeconds(10)
            do {
                Start-Sleep -Milliseconds 50
                $events = @(Get-Content -LiteralPath (Join-Path $flowRoot 'events.log'))
            } while ($events.Count -lt 5 -and [DateTime]::UtcNow -lt $deadline)
            $expected = @($expectedProvision, $expectedProvision, '--old-uninstall', 'installed-files', '--installer-commit-preferences --auto-update=enabled')
            if (($events -join "`n") -ne ($expected -join "`n")) { throw ('Unexpected retry/install/commit ordering: ' + ($events -join '; ')) }
            if (-not (Test-Path -LiteralPath (Join-Path $flowRoot 'installed.marker'))) { throw 'Successful bootstrap did not allow file installation.' }
            $next = [NsisFixtureWindow]::GetDlgItem($window, 1)
            while (-not [NsisFixtureWindow]::IsWindowEnabled($next) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
            if (-not [NsisFixtureWindow]::IsWindowEnabled($next)) { throw 'Successful preference commit did not enable continuation.' }
            # The successful GUI stub emits six known lines; Rust tests check their exact text.
            if ([NsisFlowDialog]::ProgressRows($window) -lt ($failedRows + 6)) { throw 'nsExec did not show the GUI-subsystem bootstrap progress lines.' }
            [NsisFlowDialog]::ClickButton($window, 1)
            $null = Wait-Control $window 'BITIG fixture completed successfully'
            [NsisFlowDialog]::ClickButton($window, 1)
            if (-not $flowProcess.WaitForExit(5000) -or $flowProcess.ExitCode -ne 0) { throw 'Successful retry did not finish with exit code zero.' }
        }
    }
    finally {
        if (-not $flowProcess.HasExited) { $flowProcess.Kill(); $flowProcess.WaitForExit() }
        $flowProcess.Dispose()
    }
}
Write-Output 'NSIS flow behavior passed: failure/cancel, retry, packaged GUI bootstrap progress, deferred prior uninstall, and final preference commit (only inert fixtures; no actual installation).'
