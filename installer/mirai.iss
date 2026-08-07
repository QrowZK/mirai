; Inno Setup script for the Mirai browser Windows installer.
; The version is injected by CI via /DAppVersion=x.y.z

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{7A2B7C1E-1B2B-4E7B-9C3D-5A1F0E9D4C21}
AppName=Mirai
AppVersion={#AppVersion}
AppPublisher=Mirai
DefaultDirName={autopf}\Mirai
DefaultGroupName=Mirai
UninstallDisplayIcon={app}\mirai.exe
SetupIconFile=..\assets\mirai.ico
OutputBaseFilename=MiraiSetup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Files]
Source: "..\target\release\mirai.exe"; DestDir: "{app}"; Flags: ignoreversion
; GStreamer runtime for video/audio playback, staged by stage-gstreamer.ps1.
; libservo loads these from the exe's own directory on Windows.
Source: "..\target\release\gst-dlls\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Mirai"; Filename: "{app}\mirai.exe"
Name: "{autodesktop}\Mirai"; Filename: "{app}\mirai.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Run]
Filename: "{app}\mirai.exe"; Description: "{cm:LaunchProgram,Mirai}"; Flags: nowait postinstall skipifsilent
