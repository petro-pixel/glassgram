; Windows installer (Inno Setup 6), built by .github/workflows/release.yml:
;   cargo build --release, ffmpeg into dist\ffmpeg, then ISCC with GLASSGRAM_VERSION set.
; Paths are relative to this file.

#define AppVersion GetEnv("GLASSGRAM_VERSION")
#if AppVersion == ""
  #define AppVersion "0.0.0"
#endif

[Setup]
; Keep this id: Windows matches upgrades and the uninstaller by it.
AppId={{C8CA7836-2DCF-41F2-9E27-BF39F48D70DB}
AppName=Glassgram
AppVersion={#AppVersion}
AppPublisher=petro-pixel
; Code signing (SignPath) checks these against the release version.
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Glassgram
VersionInfoProductVersion={#AppVersion}
VersionInfoProductTextVersion={#AppVersion}
AppPublisherURL=https://github.com/petro-pixel/glassgram
AppSupportURL=https://github.com/petro-pixel/glassgram/issues
DefaultDirName={autopf}\Glassgram
DefaultGroupName=Glassgram
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=Glassgram-{#AppVersion}-setup
SetupIconFile=..\icons\glassgram.ico
UninstallDisplayIcon={app}\glassgram.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Per-user install without admin rights is offered too.
PrivilegesRequiredOverridesAllowed=dialog
; Glassgram must not be running while files are replaced.
CloseApplications=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "uk"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\glassgram.exe"; DestDir: "{app}"; Flags: ignoreversion
; ffmpeg next to the exe is where platform::tool looks first.
Source: "..\..\dist\ffmpeg\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\ffmpeg\ffplay.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\ffmpeg\LICENSE.txt"; DestDir: "{app}"; DestName: "FFMPEG-LICENSE.txt"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Glassgram"; Filename: "{app}\glassgram.exe"
Name: "{autodesktop}\Glassgram"; Filename: "{app}\glassgram.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\glassgram.exe"; Description: "{cm:LaunchProgram,Glassgram}"; Flags: nowait postinstall skipifsilent

; The session and settings in %APPDATA%\glassgram stay after uninstalling, as with Telegram Desktop.
