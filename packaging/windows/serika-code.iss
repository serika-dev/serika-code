; Windows installer for Serika Code (Inno Setup 6).
;
; Built by scripts\package-qt.ps1 -Installer, which passes the version, the packaged app folder
; and the installer pictures:
;
;   ISCC /DAppVersion=0.2.0 /DSourceDir=dist\serika-code /DArtDir=dist\installer-art
;        /DOutputDir=dist packaging\windows\serika-code.iss
;
; Installs for the current user by default (no admin prompt, into
; %LOCALAPPDATA%\Programs\Serika Code); the first page offers an install for everyone instead.
; Updates install over the old version in place; chats and settings live elsewhere and are kept
; on uninstall.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\serika-code"
#endif
#ifndef ArtDir
  #define ArtDir "..\..\dist\installer-art"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define AppName "Serika Code"
#define AppExe "serika-code.exe"
#define AppUrl "https://github.com/serika-dev/serika-code"

[Setup]
AppId={{C527ADDF-E945-4D32-93C0-57FF3EB9552B}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Serika
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
AppCopyright=Apache-2.0
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} installer

DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableWelcomePage=no
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=no
UsePreviousAppDir=yes
UsePreviousTasks=yes

PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

OutputDir={#OutputDir}
OutputBaseFilename=serika-code-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\qt\serika-code\app\serika-code.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}

WizardStyle=modern
WizardSizePercent=110
WizardImageFile={#ArtDir}\wizard-large-100.bmp,{#ArtDir}\wizard-large-200.bmp
WizardSmallImageFile={#ArtDir}\wizard-small-100.bmp,{#ArtDir}\wizard-small-200.bmp
WizardImageStretch=no

Compression=lzma2/ultra64
SolidCompression=yes
LZMANumBlockThreads=4

; A running copy is closed for the update and nothing asks for a reboot.
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no
ChangesEnvironment=yes
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
WelcomeLabel2=This will install [name/ver] on your computer.%n%nSerika Code is a fast, native coding agent: chat with any model, let it read and edit your project, run commands and browse the web, and review every change.%n%nClose Serika Code before you continue.
FinishedLabel=Serika Code is installed. Open it from the Start menu, or type serika in a new terminal.

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"
Name: "addtopath"; Description: "Add the &serika command to PATH (use it from any terminal)"; GroupDescription: "Command line:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; Qt files from an older version that this one no longer ships
Type: filesandordirs; Name: "{app}\qml"
Type: filesandordirs; Name: "{app}\platforms"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{userdocs}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{userdocs}"; Tasks: desktopicon

[Registry]
; "serika-code" from Run (Win+R) and the start of a command line
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\serika-code.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\serika-code.exe"; ValueType: string; ValueName: "Path"; ValueData: "{app}"

[Run]
Filename: "{app}\{#AppExe}"; Description: "Open {#AppName}"; WorkingDir: "{userdocs}"; Flags: nowait postinstall skipifsilent


[Code]
const
  UserEnv = 'Environment';
  MachineEnv = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

function EnvRoot: Integer;
begin
  if IsAdminInstallMode then Result := HKEY_LOCAL_MACHINE else Result := HKEY_CURRENT_USER;
end;

function EnvKey: String;
begin
  if IsAdminInstallMode then Result := MachineEnv else Result := UserEnv;
end;

// Whether dir is one of the entries of a ;-separated PATH (case does not matter).
function PathHas(const Path, Dir: String): Boolean;
begin
  Result := Pos(';' + Lowercase(Dir) + ';', ';' + Lowercase(Path) + ';') > 0;
end;

procedure AddToPath(const Dir: String);
var
  Path: String;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Path) then Path := '';
  if PathHas(Path, Dir) then Exit;
  if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then Path := Path + ';';
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', Path + Dir);
end;

procedure RemoveFromPath(const Dir: String);
var
  Path: String;
  P: Integer;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Path) then Exit;
  Path := ';' + Path + ';';
  P := Pos(';' + Lowercase(Dir) + ';', Lowercase(Path));
  if P = 0 then Exit;
  Delete(Path, P, Length(Dir) + 1);
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

// The uninstaller cannot remove files of a copy that is still open: close the app and its
// runtime first, but only the ones started from this install folder.
procedure CloseRunningCopies;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-Process serika-code,serika -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -like ''' + ExpandConstant('{app}') + '\*'' } | Stop-Process -Force"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    CloseRunningCopies;
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
