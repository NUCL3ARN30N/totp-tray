; Built by .github/workflows/release.yml; AppVersion is passed with /DAppVersion=x.y.z
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{B7C1E5A2-6D3F-4C1B-9A57-3E0F2D8A4C61}
AppName=TOTP Tray
AppVersion={#AppVersion}
AppPublisher=NUCL3ARN30N
DefaultDirName={autopf}\TOTP Tray
DefaultGroupName=TOTP Tray
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\dist
OutputBaseFilename=totp-tray-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\totp-tray.exe

[Files]
Source: "..\target\release\totp-tray.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\TOTP Tray"; Filename: "{app}\totp-tray.exe"
Name: "{autostartup}\TOTP Tray"; Filename: "{app}\totp-tray.exe"; Tasks: autostart

[Tasks]
Name: "autostart"; Description: "Start TOTP Tray when I sign in"; Flags: unchecked

[Run]
Filename: "{app}\totp-tray.exe"; Description: "Launch TOTP Tray"; Flags: nowait postinstall skipifsilent
