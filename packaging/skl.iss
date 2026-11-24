#define AppVer GetEnv("SKL_VERSION")
[Setup]
AppId={{6B1E3C1A-5A4E-4C8E-9D3B-5C1A0F0E5F11}
AppName=SosiskaKillerLauncher
AppVersion={#AppVer}
AppPublisher=SKL
DefaultDirName={localappdata}\Programs\SosiskaKillerLauncher
DefaultGroupName=SosiskaKillerLauncher
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=SosiskaKillerLauncher-Setup-{#AppVer}
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\SosiskaKillerLauncher.exe
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "ukrainian"; MessagesFile: "compiler:Languages\Ukrainian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\SosiskaKillerLauncher.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\SosiskaKillerLauncher"; Filename: "{app}\SosiskaKillerLauncher.exe"
Name: "{group}\{cm:UninstallProgram,SosiskaKillerLauncher}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SosiskaKillerLauncher"; Filename: "{app}\SosiskaKillerLauncher.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\SosiskaKillerLauncher.exe"; Description: "{cm:LaunchProgram,SosiskaKillerLauncher}"; Flags: nowait postinstall skipifsilent
