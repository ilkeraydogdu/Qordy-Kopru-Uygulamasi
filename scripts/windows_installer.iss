; ==============================================================================
; QORDY Smart Print Bridge - Enterprise Inno Setup Script for Windows
; Produces: Qordy_Bridge_Setup_v2.5.0.exe
; ==============================================================================

#define MyAppName "QORDY Smart Print Bridge"
#define MyAppVersion "2.5.1"
#define MyAppPublisher "Pofuduk Dijital Medya ve Yazilim Ltd. Sti."
#define MyAppURL "https://qordy.com"
#define MyAppExeName "qordy_bridge_rs.exe"

[Setup]
AppId={{D37F8E19-7A8B-4C9D-9F3E-81624B5D0F21}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\Qordy\Bridge
DefaultGroupName=QORDY
AllowNoIcons=yes
OutputDir=..\dist
OutputBaseFilename=Qordy_Bridge_v{#MyAppVersion}_Windows_Setup
SetupIconFile=..\assets\favicon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
CloseApplications=yes
RestartApplications=no
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "Windows baslangicinda otomatik calistir (Tavsiye Edilir)"; GroupDescription: "Sistem Secenekleri:"

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\config.json"; DestDir: "{app}"; Flags: ignoreversion onlyifdoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Qordy Yerel Panel"; Filename: "http://localhost:8089"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "QordyPrintBridge"; ValueData: """{app}\{#MyAppExeName}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
