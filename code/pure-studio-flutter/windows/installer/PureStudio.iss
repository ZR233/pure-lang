#ifndef MyAppVersion
  #error MyAppVersion is required
#endif
#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef OutputBase
  #error OutputBase is required
#endif

#define MyAppName "Pure Studio"
#define MyAppPublisher "Pure-Lang"
#define MyAppExeName "pure_studio_flutter.exe"

[Setup]
AppId={{B17C2A25-1255-4D18-8D8C-55CA4E8F06C4}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\Pure Studio
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBase}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=yes
SetupLogging=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
VersionInfoVersion={#MyAppVersion}.0
VersionInfoProductName={#MyAppName}
VersionInfoDescription={#MyAppName} Installer
LicenseFile={#SourceDir}\LICENSE

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "*.pdb"

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Dirs]
Name: "{localappdata}\Pure Studio\crashes"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: expandsz; ValueName: "DumpFolder"; ValueData: "{localappdata}\Pure Studio\crashes"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: dword; ValueName: "DumpType"; ValueData: "2"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: dword; ValueName: "DumpCount"; ValueData: "10"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
