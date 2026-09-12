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

#define MyAppName "anywork"
#define MyAppPublisher "Pure-Lang"
#define MyAppExeName "anywork.exe"

[Setup]
AppId={{701A3525-12D7-4D51-A83E-8A62EE6F3060}
AppName={cm:AppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\anywork
DefaultGroupName={cm:AppName}
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
SetupIconFile=..\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
VersionInfoVersion={#MyAppVersion}.0
VersionInfoProductName={#MyAppName}
VersionInfoDescription={#MyAppName} Installer
LicenseFile={#SourceDir}\LICENSE

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "zh"; MessagesFile: "compiler:Default.isl"
Name: "zhTW"; MessagesFile: "compiler:Default.isl"

[LangOptions]
zh.LanguageName=简体中文
zh.LanguageID=$0804
zhTW.LanguageName=繁體中文
zhTW.LanguageID=$0404

[CustomMessages]
en.AppName=anywork
zh.AppName=糊来帮
zhTW.AppName=糊来帮
en.CreateDesktopIcon=Create a desktop shortcut
zh.CreateDesktopIcon=创建桌面快捷方式
zhTW.CreateDesktopIcon=建立桌面捷徑
en.AdditionalIcons=Additional icons:
zh.AdditionalIcons=附加图标：
zhTW.AdditionalIcons=附加圖示：
en.LaunchApp=Launch %1
zh.LaunchApp=启动 %1
zhTW.LaunchApp=啟動 %1

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "*.pdb"

[Icons]
Name: "{autoprograms}\{cm:AppName}"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "io.github.zr233.anywork"
Name: "{autodesktop}\{cm:AppName}"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "io.github.zr233.anywork"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Dirs]
Name: "{localappdata}\anywork\crashes"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: expandsz; ValueName: "DumpFolder"; ValueData: "{localappdata}\anywork\crashes"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: dword; ValueName: "DumpType"; ValueData: "2"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\{#MyAppExeName}"; ValueType: dword; ValueName: "DumpCount"; ValueData: "10"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchApp,{cm:AppName}}"; Flags: nowait postinstall skipifsilent
