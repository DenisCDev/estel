#define AppVersion GetEnv("ESTEL_VERSION")

[Setup]
AppId=DenisCDev.Estel
AppName=Estel
AppVersion={#AppVersion}
AppPublisher=Denis Scarabelli
AppPublisherURL=https://github.com/DenisCDev/estel
AppSupportURL=https://github.com/DenisCDev/estel/issues
DefaultDirName={localappdata}\Programs\Estel
DefaultGroupName=Estel
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\artifacts
OutputBaseFilename=Estel-Setup-x86_64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayName=Estel
UninstallDisplayIcon={app}\estel.exe
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Instalador do Estel
VersionInfoCompany=Denis Scarabelli
VersionInfoProductName=Estel

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Files]
Source: "..\target\release\estel.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Estel"; Filename: "{app}\estel.exe"; Parameters: "--settings"; WorkingDir: "{app}"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "Estel"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\estel.exe"; Parameters: "--settings"; Description: "Abrir Estel"; Flags: nowait postinstall skipifsilent
