; Inno Setup 6 script: a per-user installer around the nana-packager layout.
; Build with scripts/package.ps1, which fills the defines below.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\app\NanaType"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{6F8A2C1E-3B7D-4E59-9C0A-4E414E415459}
AppName=NanaType
AppVersion={#AppVersion}
AppPublisher=sena-nana
AppPublisherURL=https://github.com/sena-nana/NanaType
DefaultDirName={localappdata}\Programs\NanaType
DefaultGroupName=NanaType
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=NanaType-{#AppVersion}-setup
SetupIconFile=..\app\assets\nanatype.ico
UninstallDisplayIcon={app}\NanaType.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Languages]
; Inno Setup ships Chinese only as an unofficial translation; a copy is kept in packaging\inno.
Name: "chinesesimplified"; MessagesFile: "inno\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "Launch NanaType at login"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; THIRD_PARTY_LICENSES.txt is already part of the validated package payload.

[Icons]
Name: "{group}\NanaType"; Filename: "{app}\NanaType.exe"
Name: "{autodesktop}\NanaType"; Filename: "{app}\NanaType.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "NanaType"; ValueData: """{app}\NanaType.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\NanaType.exe"; Description: "{cm:LaunchProgram,NanaType}"; Flags: nowait postinstall skipifsilent

; User data (settings, skins, logs) lives under %APPDATA% / %LOCALAPPDATA%
; and is kept on uninstall.
