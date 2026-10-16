; Inno Setup script for the LeVerbe Parallel Windows installer.
;   iscc /DAppVersion=1.2.3 /DSourceDir=<staged app dir> /DOutputDir=<dir> leverbe.iss
; <staged app dir> holds LeVerbe ParallelParallel.exe and data\bible.db (see .github/workflows/release.yml).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\LeVerbe Parallel"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{A208954B-F4C3-4991-916C-68FE32C7F9A1}
AppName=LeVerbe Parallel
AppVersion={#AppVersion}
AppPublisher=LeVerbe Parallel
AppPublisherURL=https://github.com/rmpr/logos-rs
DefaultDirName={autopf}\LeVerbeParallel
DefaultGroupName=LeVerbe Parallel
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\LeVerbe ParallelParallel.exe
OutputDir={#OutputDir}
OutputBaseFilename=LeVerbe ParallelParallel-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\LeVerbe ParallelParallel.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\data\bible.db"; DestDir: "{app}\data"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\LeVerbe Parallel"; Filename: "{app}\LeVerbe ParallelParallel.exe"
Name: "{autodesktop}\LeVerbe Parallel"; Filename: "{app}\LeVerbe ParallelParallel.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\LeVerbe ParallelParallel.exe"; Description: "{cm:LaunchProgram,LeVerbe Parallel}"; Flags: nowait postinstall skipifsilent
