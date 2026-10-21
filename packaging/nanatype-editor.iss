; Inno Setup 6 script: a per-user installer for NanaType Editor around the
; nana-packager layout. Build with scripts/package.ps1 -Product editor, which
; fills the defines below. NanaType's own installer is nanatype.iss.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\app\NanaTypeEditor"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
AppId={{5AFB1CB3-537C-4CF2-8C98-7185BC0ED1DF}
AppName=NanaType Editor
AppVersion={#AppVersion}
AppPublisher=sena-nana
AppPublisherURL=https://github.com/sena-nana/NanaType
DefaultDirName={localappdata}\Programs\NanaType Editor
DefaultGroupName=NanaType Editor
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=NanaTypeEditor-{#AppVersion}-setup
SetupIconFile=..\editor\assets\nanatype-editor.ico
UninstallDisplayIcon={app}\NanaTypeEditor.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; THIRD_PARTY_LICENSES.txt is already part of the validated package payload.

[Icons]
Name: "{group}\NanaType Editor"; Filename: "{app}\NanaTypeEditor.exe"
Name: "{autodesktop}\NanaType Editor"; Filename: "{app}\NanaTypeEditor.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\NanaTypeEditor.exe"; Description: "{cm:LaunchProgram,NanaType Editor}"; Flags: nowait postinstall skipifsilent

; Drafts and the signing key live in NanaType's data folder (%APPDATA%) and
; are kept on uninstall, like NanaType's own data.
