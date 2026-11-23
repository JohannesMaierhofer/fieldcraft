[Setup]
AppId=FieldCraft-NGSolve
AppName=FieldCraft with NGSolve
AppVersion=0.8.1
DefaultDirName={localappdata}\Programs\FieldCraft-NGSolve
DefaultGroupName=FieldCraft with NGSolve
PrivilegesRequired=lowest
ArchitecturesAllowed={#Allowed}
ArchitecturesInstallIn64BitMode={#Allowed}
UninstallDisplayIcon={app}\fieldcraft.exe
OutputBaseFilename=fieldcraft-ngsolve-{#Label}
Compression=lzma2/fast
SolidCompression=yes

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\FieldCraft"; Filename: "{app}\fieldcraft.exe"
