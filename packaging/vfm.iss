; Inno Setup script for VFM.
;
; Build the portable folder first (tools/package.ps1), then either let the script compile this
; file or run ISCC with /DAppVersion=… /DSourceDir=… /DOutputDir=…

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\VFM-0.1.0"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{7B2F3C64-1D4A-4E2B-9C6F-1E7A5B0D3A21}
AppName=VFM
AppVersion={#AppVersion}
AppPublisher=VFM
DefaultDirName={autopf}\VFM
DefaultGroupName=VFM
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=VFM-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\VFM.exe
LicenseFile={#SourceDir}\THIRD_PARTY_LICENSES.md

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "arabic"; MessagesFile: "compiler:Languages\Arabic.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associate"; Description: "Associate video and audio files with VFM"; GroupDescription: "File associations:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs

[Icons]
Name: "{group}\VFM"; Filename: "{app}\VFM.exe"
Name: "{group}\{cm:UninstallProgram,VFM}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\VFM"; Filename: "{app}\VFM.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\VFM.exe"; Parameters: "--associate"; Flags: runhidden; Tasks: associate
Filename: "{app}\VFM.exe"; Description: "{cm:LaunchProgram,VFM}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\VFM.exe"; Parameters: "--unassociate"; Flags: runhidden

[Registry]
Root: HKCU; Subkey: "Software\VFM"; ValueType: string; ValueName: "InstallDir"; ValueData: "{app}"; Flags: uninsdeletekey
