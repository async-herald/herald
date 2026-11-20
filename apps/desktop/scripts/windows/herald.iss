; Installeur Windows d'Async Herald (Inno Setup 6).
;
;   ISCC.exe /DAppVersion=0.2.0 scripts\windows\herald.iss   (depuis apps\desktop)
;   → dist\Async-Herald-Setup-0.2.0.exe
;
; Installation pour l'utilisateur courant, sans droits administrateur
; (%LOCALAPPDATA%\Programs\Async Herald) : une mise à jour se réinstalle par-dessus
; sans demander de mot de passe administrateur. Les données de l'app restent dans
; %APPDATA%\fr.itsasync.herald.desktop, conservées à la désinstallation.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  ; Binaire du workspace Cargo : target\ à la racine du dépôt.
  #define SourceExe "..\..\..\..\target\release\herald-desktop.exe"
#endif

[Setup]
; Identifiant fixe : ne jamais le changer, il relie les versions entre elles.
AppId={{7B3C2E8A-5D41-4F6B-9A2E-8C1D4E7F9B30}
AppName=Async Herald
AppVersion={#AppVersion}
AppVerName=Async Herald {#AppVersion}
AppPublisher=Its Async
AppPublisherURL=https://herald.codes
AppSupportURL=https://herald.codes
AppUpdatesURL=https://herald.codes
DefaultDirName={autopf}\Async Herald
DefaultGroupName=Async Herald
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\..\dist
OutputBaseFilename=Async-Herald-Setup-{#AppVersion}
SetupIconFile=..\..\assets\windows\herald.ico
UninstallDisplayIcon={app}\herald-desktop.exe
UninstallDisplayName=Async Herald
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Ferme l'app si elle tourne pendant une mise à jour.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "herald-desktop.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Async Herald"; Filename: "{app}\herald-desktop.exe"
Name: "{autodesktop}\Async Herald"; Filename: "{app}\herald-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\herald-desktop.exe"; Description: "{cm:LaunchProgram,Async Herald}"; Flags: nowait postinstall skipifsilent
