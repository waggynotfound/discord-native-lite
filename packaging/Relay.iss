; Build through packaging/build-release.ps1 with Inno Setup 6.3+.
#ifndef AppVersion
  #error Missing AppVersion
#endif
#ifndef PayloadDir
  #error Missing PayloadDir
#endif

[Setup]
AppId={{7128B448-EEE8-49F5-B7A1-336AC8D4FC8A}
AppName=Relay
AppVersion={#AppVersion}
AppPublisher=Relay contributors
AppPublisherURL=https://github.com/waggynotfound/discord-native-lite
AppSupportURL=https://github.com/waggynotfound/discord-native-lite/issues
AppUpdatesURL=https://github.com/waggynotfound/discord-native-lite/releases
DefaultDirName={localappdata}\Programs\Relay
DefaultGroupName=Relay
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.14393
OutputBaseFilename=Relay-{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\relay.ico
LicenseFile={#PayloadDir}\LICENSE
UninstallDisplayIcon={app}\Relay.exe
UninstallDisplayName=Relay
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName=Relay
VersionInfoCompany=Relay contributors
VersionInfoDescription=Relay for Windows setup
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=no
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Relay"; Filename: "{app}\Relay.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\Relay"; Filename: "{app}\Relay.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\Relay.exe"; Description: "Open Relay"; Flags: nowait postinstall skipifsilent unchecked
