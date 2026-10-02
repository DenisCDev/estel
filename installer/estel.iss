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
#ifdef EstelSignedBuild
SignTool=estel_release
SignedUninstaller=yes
SignToolRetryCount=0
#endif

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Files]
#ifdef EstelSignedBuild
Source: "..\target\release\estel.exe"; DestDir: "{app}"; Flags: ignoreversion sign
#else
Source: "..\target\release\estel.exe"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Icons]
Name: "{autoprograms}\Estel"; Filename: "{app}\estel.exe"; Parameters: "--settings"; WorkingDir: "{app}"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Estel"; ValueData: """{app}\estel.exe"""; Flags: uninsdeletevalue; Check: EnableAutostart

[Run]
Filename: "{app}\estel.exe"; Parameters: "--settings"; Description: "Configurar e abrir Estel (localização, janela e câmera opcionais)"; Flags: nowait postinstall skipifsilent

[Code]
var
  PreviousInstall: Boolean;

function InitializeSetup(): Boolean;
begin
  PreviousInstall := RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\DenisCDev.Estel_is1');
  Result := True;
end;

function EnableAutostart(): Boolean;
var
  ExistingCommand: string;
begin
  Result := (not PreviousInstall) or
    RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Estel', ExistingCommand);
end;
