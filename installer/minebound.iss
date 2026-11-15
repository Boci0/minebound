; Windows installer, built by .github/workflows/release.yml:
;   iscc /DVersion=0.2.0 installer\minebound.iss
; Installs for the current user only (no admin prompt), with a Start menu entry, an optional
; desktop shortcut and an uninstaller. Running a newer setup upgrades the installed game in place.

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
; never change AppId: it is how a new setup finds the installed game to upgrade
AppId={{6E0B5A3C-3F2D-4C8E-9A51-7D4B2E9C1F08}
AppName=Minebound
AppVersion={#Version}
AppPublisher=Boci0
AppPublisherURL=https://github.com/Boci0/minebound
DefaultDirName={localappdata}\Programs\Minebound
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Minebound-setup
UninstallDisplayIcon={app}\minebound.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SourceDir=..

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"

[InstallDelete]
; drop music from older versions so an upgrade leaves no stale files
Type: filesandordirs; Name: "{app}\assets"

[Files]
Source: "target\release\minebound.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "assets\music\*"; DestDir: "{app}\assets\music"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Minebound"; Filename: "{app}\minebound.exe"
Name: "{autodesktop}\Minebound"; Filename: "{app}\minebound.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\minebound.exe"; Description: "Play Minebound"; Flags: nowait postinstall skipifsilent
