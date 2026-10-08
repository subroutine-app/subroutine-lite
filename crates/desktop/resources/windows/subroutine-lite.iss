#ifndef ResourcesDir
  #error ResourcesDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef Version
  #error Version is required
#endif
#ifndef Architecture
  #error Architecture is required
#endif
#if (Architecture != "x86_64") && (Architecture != "aarch64")
  #error Architecture must be x86_64 or aarch64
#endif

[Setup]

AppId={{5E846D83-AC4C-4F76-9922-41838D0F0C3D}
AppName=Subroutine Lite
AppVersion={#Version}
AppPublisher=Subroutine
DefaultDirName={localappdata}\Programs\Subroutine Lite
DefaultGroupName=Subroutine Lite
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
UninstallDisplayName=Subroutine Lite
UninstallDisplayIcon={app}\subroutine-lite.exe
SetupIconFile={#ResourcesDir}\Subroutine.ico
OutputDir={#OutputDir}
OutputBaseFilename=Subroutine-Lite-{#Version}-{#Architecture}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
#if Architecture == "aarch64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
VersionInfoProductName=Subroutine Lite
VersionInfoDescription=Subroutine Lite Setup
VersionInfoVersion={#Version}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ResourcesDir}\subroutine-lite.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ResourcesDir}\notices\*"; DestDir: "{app}\notices"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Subroutine Lite"; Filename: "{app}\subroutine-lite.exe"; WorkingDir: "{app}"; AppUserModelID: "com.subroutine.SubroutineLite"
Name: "{userdesktop}\Subroutine Lite"; Filename: "{app}\subroutine-lite.exe"; WorkingDir: "{app}"; AppUserModelID: "com.subroutine.SubroutineLite"; Tasks: desktopicon

[Run]
Filename: "{app}\subroutine-lite.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,Subroutine Lite}"; Flags: nowait postinstall skipifsilent
; Silent installation launches only when the local -Install workflow requests it.
Filename: "{app}\subroutine-lite.exe"; WorkingDir: "{app}"; Flags: nowait runasoriginaluser; Check: LaunchAfterInstall

[Code]
function LaunchAfterInstall: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:LaunchAfterInstall|0}') = '1');
end;
