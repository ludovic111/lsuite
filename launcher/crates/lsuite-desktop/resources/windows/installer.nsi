; The launcher's Windows installer (scripts/bundle-windows.sh runs makensis on it):
;   makensis /DVERSION=0.1.0 /DSRC=<folder with lsuite.exe…> /DOUTFILE=<setup.exe> installer.nsi
;
; Per-user install into %LOCALAPPDATA%\lsuite (no administrator), a Start menu shortcut and an
; entry in Settings › Apps. `/P` skips the pages (passive, for the updater), `/R` starts lsuite
; when done; /S is NSIS's silent mode.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define PRODUCT "lsuite"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "lsuite ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "lsuite installer"
VIAddVersionKey "LegalCopyright" "MIT licence"

!define MUI_ICON "${SRC}\lsuite.ico"
!define MUI_UNICON "${SRC}\lsuite.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\lsuite.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open lsuite"

Var Passive

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  StrCpy $Passive 0
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
    SetAutoClose true
  ${EndIf}
FunctionEnd

Function SkipWhenPassive
  ${If} $Passive == 1
    Abort
  ${EndIf}
FunctionEnd

Section "lsuite" Main
  SetOutPath "$INSTDIR"
  ; The launcher may still be closing (its updater quits it just before running this).
  Sleep 500
  SetOverwrite on
  File "${SRC}\lsuite.exe"
  File "${SRC}\lsuite-cli.exe"
  File "${SRC}\lsuite-mcp.exe"
  File "${SRC}\lsuite.ico"
  File "${SRC}\LICENSE.txt"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\lsuite.exe" "" "$INSTDIR\lsuite.ico" 0

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\lsuite.ico$\""
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "lsuite"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://lsuite.xyz/launcher"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" $0

  ; /R (from the updater): start lsuite again.
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\lsuite.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\lsuite.exe"
  Delete "$INSTDIR\lsuite-cli.exe"
  Delete "$INSTDIR\lsuite-mcp.exe"
  Delete "$INSTDIR\lsuite.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"
  ; The apps it installed and their plugins (%USERPROFILE%\.lsuite) stay.
SectionEnd
