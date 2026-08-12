#define DefaultVersion "0.2.0"
#ifndef MyAppVersion
  #define MyAppVersion DefaultVersion
#endif

[Setup]
AppId={{B9F59A98-0D1B-4C18-B03D-4C6F2A6A7F42}
AppName=AlterSendme
AppVersion={#MyAppVersion}
AppPublisher=BruceBlink
DefaultDirName={autopf}\AlterSendme
DefaultGroupName=AlterSendme
OutputBaseFilename=AlterSendme-{#MyAppVersion}-windows-setup
OutputDir=.
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
UninstallDisplayIcon={app}\AlterSendme.exe
PrivilegesRequired=lowest

[Files]
Source: "..\artifacts\installer-input\AlterSendme.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: isreadme

[Icons]
Name: "{group}\AlterSendme"; Filename: "{app}\AlterSendme.exe"
Name: "{autodesktop}\AlterSendme"; Filename: "{app}\AlterSendme.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"

[Run]
Filename: "{app}\AlterSendme.exe"; Description: "Launch AlterSendme"; Flags: nowait postinstall skipifsilent
