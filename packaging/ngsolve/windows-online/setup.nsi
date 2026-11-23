Unicode true
RequestExecutionLevel user
ManifestSupportedOS Win10
!include "FileFunc.nsh"
VIProductVersion 0.8.1.2
VIAddVersionKey ProductName "FieldCraft with NGSolve"
VIAddVersionKey ProductVersion "0.8.1-r2"
VIAddVersionKey FileVersion "0.8.1-r2"
VIAddVersionKey FileDescription "FieldCraft with NGSolve network installer"
VIAddVersionKey LegalCopyright "FieldCraft contributors and their respective licensors"
Name "FieldCraft 0.8.1 with NGSolve (r2 network installer, ${ARCH})"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\FieldCraft-NGSolve-Network"
SetCompressor /FINAL zlib
ShowInstDetails show
Page directory
Page instfiles

Section
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  File "${PAYLOAD}/fieldcraft.exe"
  File "${PAYLOAD}/fieldcraft-cli.exe"
  File "${PAYLOAD}/identity.json"
  File "${PAYLOAD}/runtime-lock.json"
  File "${PAYLOAD}/SOLVER-NOTICES.txt"
  File "${PAYLOAD}/INSTALL.txt"
  File "${PAYLOAD}/fieldcraft-ngsolve.cmd"
  File "${PAYLOAD}/install.ps1"
  SetOutPath "$PLUGINSDIR\payload\licenses"
  File "${PAYLOAD}/licenses/LICENSE-MIT"
  File "${PAYLOAD}/licenses/LICENSE-APACHE"
  File "${PAYLOAD}/licenses/NOTICE"
  File "${PAYLOAD}/licenses/THIRD-PARTY-LICENSES.txt"
  File "${PAYLOAD}/licenses/NSIS.txt"
  SetOutPath "$PLUGINSDIR\payload"
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/NOSHORTCUTS" $1
  IfErrors normal
  StrCpy $2 "-SkipShortcuts"
  Goto install
normal:
  StrCpy $2 ""
install:
  DetailPrint "Installing into $INSTDIR. Runtime downloads are verified against SHA-256."
  ClearErrors
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "$PLUGINSDIR\payload\install.ps1" -InstallRoot "$INSTDIR" $2' $3
  IfErrors failed
  StrCmp $3 "0" done
failed:
  MessageBox MB_ICONSTOP "Installation failed. See %TEMP%\FieldCraft-NGSolve-network-*.log for the cause." /SD IDOK
  SetErrorLevel 1
  Abort
done:
  SetErrorLevel 0
SectionEnd
