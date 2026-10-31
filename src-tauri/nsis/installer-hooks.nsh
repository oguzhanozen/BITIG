!ifndef BITIG_INSTALLER_HOOKS
!define BITIG_INSTALLER_HOOKS

Var BitigTermsAccepted
Var BitigAutoUpdate
Var BitigAutoUpdateCheckbox
Var BitigYtDlpCheckbox
Var BitigFfmpegCheckbox
Var BitigPreviousUninstaller
Var BitigPreviousDirectory
Var BitigBootstrapArguments
Var BitigBootstrapResult

ShowInstDetails show

!macro BITIG_LICENSE_PAGE
  !define MUI_LICENSEPAGE_CHECKBOX
  !define MUI_LICENSEPAGE_CHECKBOX_TEXT "I accept the BITIG Terms of Use and have read the Third-Party Software Notices."
  !define MUI_PAGE_CUSTOMFUNCTION_LEAVE BitigLicenseLeave
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!macroend

!macro BITIG_INIT
  Call BitigRejectUnattended
  StrCpy $PassiveMode 0
  StrCpy $BitigTermsAccepted 0
  StrCpy $BitigAutoUpdate "disabled"
  StrCpy $BitigPreviousUninstaller ""
!macroend

!macro BITIG_CHECK_INSTALL_ERRORS
  ${If} ${Errors}
    SetErrorLevel 1
    Abort "BITIG Setup could not write the application files or registration."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  Call BitigRunPreviousUninstaller
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro BITIG_CHECK_INSTALL_ERRORS
  StrCpy $BitigBootstrapArguments "--installer-commit-preferences --auto-update=$BitigAutoUpdate"
  Call BitigRunBootstrap
  Delete "$PLUGINSDIR\bitig-bootstrap.exe"
!macroend

!macro BITIG_FUNCTIONS
!insertmacro BITIG_COMMAND_LINE_FUNCTION

Function BitigLicenseLeave
  FindWindow $0 "#32770" "" $HWNDPARENT
  ; NSIS 3.11's native LicenseForceSelection checkbox; retest on NSIS upgrades.
  GetDlgItem $0 $0 1034
  SendMessage $0 ${BM_GETCHECK} 0 0 $1
  StrCpy $BitigTermsAccepted 0
  ${If} $1 != ${BST_CHECKED}
    Abort
  ${EndIf}
  StrCpy $BitigTermsAccepted 1
FunctionEnd

Function BitigMediaToolsPage
  !insertmacro MUI_HEADER_TEXT "Media Tools and Updates" "Review required media tools and optional update settings."
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    SetErrorLevel 1
    Quit
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 32u "BITIG requires yt-dlp, FFmpeg, and ffprobe for its core media features. Setup reuses existing verified BITIG tools or securely downloads and verifies missing components. Automatic tool updates are optional."
  Pop $0
  ${NSD_CreateLabel} 0 36u 100% 12u "Required components"
  Pop $0
  ${NSD_CreateCheckbox} 8u 50u -16u 14u "yt-dlp — Video and audio downloading"
  Pop $BitigYtDlpCheckbox
  ${NSD_Check} $BitigYtDlpCheckbox
  EnableWindow $BitigYtDlpCheckbox 0
  ${NSD_CreateCheckbox} 8u 68u -16u 14u "FFmpeg + ffprobe — Media processing and analysis"
  Pop $BitigFfmpegCheckbox
  ${NSD_Check} $BitigFfmpegCheckbox
  EnableWindow $BitigFfmpegCheckbox 0
  ${NSD_CreateCheckbox} 8u 96u -16u 14u "Automatically update media tools"
  Pop $BitigAutoUpdateCheckbox
  ${If} $BitigAutoUpdate == "enabled"
    ${NSD_Check} $BitigAutoUpdateCheckbox
  ${Else}
    ${NSD_Uncheck} $BitigAutoUpdateCheckbox
  ${EndIf}
  ${NSD_OnClick} $BitigAutoUpdateCheckbox BitigUpdateChoiceClicked
  ${NSD_SetFocus} $BitigAutoUpdateCheckbox
  nsDialogs::Show
FunctionEnd

Function BitigUpdateChoiceClicked
  Pop $0
  Call BitigSaveUpdateChoice
FunctionEnd

Function BitigSaveUpdateChoice
  ${NSD_GetState} $BitigAutoUpdateCheckbox $0
  StrCpy $BitigAutoUpdate "disabled"
  ${If} $0 == ${BST_CHECKED}
    StrCpy $BitigAutoUpdate "enabled"
  ${EndIf}
FunctionEnd

Function BitigMediaToolsLeave
  ${NSD_GetState} $BitigYtDlpCheckbox $0
  ${NSD_GetState} $BitigFfmpegCheckbox $1
  ${If} $0 != ${BST_CHECKED}
  ${OrIf} $1 != ${BST_CHECKED}
  ${OrIf} $BitigTermsAccepted != 1
    Abort
  ${EndIf}
  Call BitigSaveUpdateChoice
FunctionEnd

Function BitigProvisionTools
  ${If} $BitigTermsAccepted != 1
    SetErrorLevel 2
    Abort "Explicit acceptance of the BITIG Terms of Use is required."
  ${EndIf}
  InitPluginsDir
  ClearErrors
  File "/oname=$PLUGINSDIR\bitig-bootstrap.exe" "${MAINBINARYSRCPATH}"
  !insertmacro BITIG_CHECK_INSTALL_ERRORS
  StrCpy $BitigBootstrapArguments "--installer-bootstrap --auto-update=$BitigAutoUpdate --defer-preferences"
  Call BitigRunBootstrap
FunctionEnd

Function BitigRunBootstrap
  bitig_bootstrap_retry:
    nsExec::ExecToLog '"$PLUGINSDIR\bitig-bootstrap.exe" $BitigBootstrapArguments'
    Pop $BitigBootstrapResult
    StrCmp $BitigBootstrapResult "0" bitig_bootstrap_done
    DetailPrint "Media tool preparation failed: $BitigBootstrapResult"
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "BITIG could not prepare the required media tools or save the update preference (result: $BitigBootstrapResult). Check your connection and available disk space, then retry, or cancel Setup." IDRETRY bitig_bootstrap_retry
    SetErrorLevel 1
    Abort "BITIG Setup did not complete."
  bitig_bootstrap_done:
FunctionEnd

Function BitigRememberUninstall
  ${If} $WixMode = 1
    ReadRegStr $BitigPreviousUninstaller HKLM "$R6" "UninstallString"
    StrCpy $BitigPreviousDirectory $INSTDIR
  ${Else}
    ReadRegStr $BitigPreviousDirectory SHCTX "${MANUPRODUCTKEY}" ""
    ReadRegStr $BitigPreviousUninstaller SHCTX "${UNINSTKEY}" "UninstallString"
    ; The previous Tauri uninstaller supports /P and /UPDATE; suppress data deletion.
    StrCpy $BitigPreviousUninstaller "$BitigPreviousUninstaller /P /UPDATE _?=$BitigPreviousDirectory"
  ${EndIf}
FunctionEnd

Function BitigRunPreviousUninstaller
  ${If} $BitigPreviousUninstaller == ""
    Return
  ${EndIf}
  ClearErrors
  ExecWait '$BitigPreviousUninstaller' $0
  ${If} ${Errors}
  ${OrIf} $0 != 0
  ${OrIf} ${FileExists} "$BitigPreviousDirectory\${MAINBINARYNAME}.exe"
    SetErrorLevel 1
    Abort "The previous BITIG version could not be uninstalled. Setup did not continue."
  ${EndIf}
FunctionEnd

Function .onInstFailed
  SetErrorLevel 1
  Call BitigDeleteBootstrap
FunctionEnd

Function .onGUIEnd
  Call BitigDeleteBootstrap
FunctionEnd

Function BitigDeleteBootstrap
  ${If} $PLUGINSDIR != ""
    Delete "$PLUGINSDIR\bitig-bootstrap.exe"
  ${EndIf}
FunctionEnd
!macroend

!macro BITIG_COMMAND_LINE_FUNCTION
Function BitigRejectUnattended
  IfSilent bitig_unattended 0
  StrCpy $7 $CMDLINE
  System::Call 'shell32::CommandLineToArgvW(w r7, *i .r0) p.r1'
  StrCmp $1 0 bitig_unattended
  StrCpy $2 1
  bitig_argument:
    IntCmp $2 $0 bitig_arguments_done
    IntOp $3 $2 * ${NSIS_PTR_SIZE}
    IntOp $3 $1 + $3
    System::Call '*$3(p .r4)'
    System::Call 'kernel32::lstrcpynW(w .r5, p r4, i ${NSIS_MAX_STRLEN}) p'
    StrCpy $6 $5 3
    ; NSIS treats everything after the final /D= as the install directory.
    StrCmpS $6 "/D=" bitig_arguments_done
    StrCmp $5 "/S" bitig_argument_rejected
    StrCmp $5 "/P" bitig_argument_rejected
    IntOp $2 $2 + 1
    Goto bitig_argument
  bitig_argument_rejected:
    System::Call 'kernel32::LocalFree(p r1)'
    Goto bitig_unattended
  bitig_arguments_done:
    System::Call 'kernel32::LocalFree(p r1)'
    Return
  bitig_unattended:
    StrCpy $1 "BITIG Setup cannot run in silent (/S) or passive (/P) mode because explicit interactive acceptance of the Terms of Use is required."
    DetailPrint $1
    System::Call 'kernel32::AttachConsole(i -1)'
    System::Call 'kernel32::GetStdHandle(i -12) p.r0'
    FileWrite $0 "$1$\r$\n"
    SetErrorLevel 2
    Quit
FunctionEnd
!macroend

!endif
