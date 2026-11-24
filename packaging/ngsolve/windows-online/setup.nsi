Unicode true
RequestExecutionLevel user
ManifestSupportedOS Win10
!include "FileFunc.nsh"
!ifndef REVISION
  !define REVISION 2
!endif
VIProductVersion 0.8.1.${REVISION}
VIAddVersionKey ProductName "FieldCraft with NGSolve"
VIAddVersionKey ProductVersion "0.8.1-r${REVISION}"
VIAddVersionKey FileVersion "0.8.1-r${REVISION}"
VIAddVersionKey FileDescription "FieldCraft with NGSolve network installer"
VIAddVersionKey LegalCopyright "FieldCraft contributors and their respective licensors"
Name "FieldCraft 0.8.1 with NGSolve (r${REVISION} network installer, ${ARCH})"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\FieldCraft-NGSolve-Network"
SetCompressor /FINAL zlib
ShowInstDetails show
Page directory
Page instfiles

Section
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  !cd "${PAYLOAD}"
  File "fieldcraft.exe"
  File "fieldcraft-cli.exe"
  File "identity.json"
  File "runtime-lock.json"
  File "SOLVER-NOTICES.txt"
  File "INSTALL.txt"
  File "fieldcraft-ngsolve.cmd"
  File "install.ps1"
  !if ${REVISION} == 4
    SetOutPath "$PLUGINSDIR\payload\examples"
    !cd "examples"
    File "slab.json"
    File "slab.analysis.json"
    File "potential.py"
    !cd ".."
  !endif
  SetOutPath "$PLUGINSDIR\payload\licenses"
  !cd "licenses"
  File "LICENSE-MIT"
  File "LICENSE-APACHE"
  File "NOTICE"
  File "THIRD-PARTY-LICENSES.txt"
  File "NSIS.txt"
  !cd ".."
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
