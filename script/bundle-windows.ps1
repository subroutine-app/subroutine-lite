#Requires -Version 7.0
[CmdletBinding()]
Param(
    [Alias('a')][ValidateSet('x86_64', 'aarch64')][string]$Architecture,
    [switch]$Install,
    [switch]$Offline,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'
# Check native exit codes ourselves; probes may fail.
$PSNativeCommandUseErrorActionPreference = $false
if ($Help) {
    Write-Output 'Usage: script/bundle-windows.ps1 [-Architecture x86_64|aarch64] [-Install] [-Offline] [-Help]'
    Write-Output 'Unsigned release Subroutine Lite installer; -Install also installs/updates and launches it.'
    Write-Output 'Requires PowerShell 7, Rust/rustup, Visual Studio C++ tools + Windows SDK, Inno Setup 6.3+.'
    Write-Output 'Requires Python 3.12+ (python, then python3) and cargo-about 0.9.2 with its cli feature.'
    Write-Output 'Finds Visual Studio and Inno in standard locations; also checks PATH for ISCC.exe.'
    Write-Output 'Downloads missing Rust targets/crates unless -Offline; licenses are always offline.'
    Write-Output '-Install requires a stopped app. Never force-closes apps or restarts Windows.'
    exit 0
}
if (-not $IsWindows) { throw 'bundle-windows.ps1 requires Windows.' }

function Assert-AppStopped {
    if (Get-Process -Name 'subroutine-lite' -ErrorAction SilentlyContinue) {
        throw 'Close Subroutine Lite, then rerun with -Install.'
    }
}

function Resolve-InnoSetup {
    $command = Get-Command ISCC.exe -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($command) { return $command.Source }
    foreach ($version in 7, 6) {
        foreach ($directory in $programDirs) {
            $path = Join-Path $directory "Inno Setup $version/ISCC.exe"
            if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
        }
    }
    throw 'Requires Inno Setup 6.3+: winget install --id JRSoftware.InnoSetup --exact (or add ISCC.exe to PATH).'
}

function Resolve-VsDevShell($TargetArchitecture) {
    $component = if ($TargetArchitecture -eq 'aarch64') {
        'Microsoft.VisualStudio.Component.VC.Tools.ARM64'
    } else {
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
    }
    $toolArch = if ($TargetArchitecture -eq 'aarch64') { 'arm64' } else { 'x64' }
    foreach ($directory in $programDirs) {
        $vswhere = Join-Path $directory 'Microsoft Visual Studio/Installer/vswhere.exe'
        if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { continue }
        $installation = & $vswhere -latest -products '*' -requires $component -property installationPath
        if ($LASTEXITCODE -ne 0) { throw 'Could not locate Visual Studio with vswhere.exe.' }
        if ($installation) {
            $path = Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1'
            if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
        }
    }
    foreach ($version in 2022, 2019) {
        foreach ($edition in 'Community', 'BuildTools', 'Professional', 'Enterprise') {
            foreach ($directory in $programDirs) {
                $installation = Join-Path $directory "Microsoft Visual Studio/$version/$edition"
                $path = Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1'
                $compiler = Join-Path $installation "VC/Tools/MSVC/*/bin/Host*/$toolArch/cl.exe"
                if ((Test-Path -LiteralPath $path -PathType Leaf) -and (Test-Path -Path $compiler -PathType Leaf)) {
                    return $path
                }
            }
        }
    }
    throw 'Launch-VsDevShell.ps1 not found. Requires Visual Studio C++ tools + Windows SDK (include ARM64 tools for aarch64).'
}

function Initialize-VsDevShell($Path, $TargetArchitecture, $HostArchitecture) {
    $targetArch = switch ($TargetArchitecture) { 'x86_64' { 'amd64' } 'aarch64' { 'arm64' } }
    $hostArch = switch ($HostArchitecture) { 'x86_64' { 'amd64' } 'aarch64' { 'arm64' } }
    $parameters = (Get-Command -Name $Path).Parameters
    Push-Location
    try {
        if ($parameters.ContainsKey('Arch') -and $parameters.ContainsKey('HostArch')) {
            & $Path -Arch $targetArch -HostArch $hostArch
        } else {
            $tools = Split-Path -Parent $Path
            $module = Join-Path $tools 'Microsoft.VisualStudio.DevShell.dll'
            $legacyModule = Join-Path $tools 'vsdevshell/Microsoft.VisualStudio.DevShell.dll'
            if (Test-Path -LiteralPath $legacyModule -PathType Leaf) { $module = $legacyModule }
            Import-Module -Name $module
            $installation = Split-Path -Parent (Split-Path -Parent $tools)
            Enter-VsDevShell -VsInstallPath $installation -SkipAutomaticLocation `
                -DevCmdArguments "-arch=$targetArch -host_arch=$hostArch"
        }
    } finally {
        Pop-Location
    }
}

if ($Install) { Assert-AppStopped }
$programDirs = @($env:ProgramW6432, $env:ProgramFiles, ${env:ProgramFiles(x86)}) |
    Where-Object { $_ } | Select-Object -Unique
$inno = Resolve-InnoSetup
# ISCC.exe's file version is not its compiler version.
Write-Host "Inno Setup: $inno"
$null = Get-Command cargo -CommandType Application -ErrorAction Stop
$null = Get-Command rustup -CommandType Application -ErrorAction Stop
$aboutVersion = & cargo about --version
if ($LASTEXITCODE -ne 0 -or $aboutVersion -ne 'cargo-about 0.9.2') {
    throw 'Install cargo-about first: cargo install cargo-about --version 0.9.2 --locked --features cli'
}
$python = $null
foreach ($name in @('python', 'python3')) {
    $candidate = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($candidate) {
        & $candidate.Source -c 'import sys; sys.exit(sys.version_info < (3, 12))' > $null 2>&1
        if ($LASTEXITCODE -eq 0) {
            $python = $candidate.Source
            break
        }
    }
}
if (-not $python) { throw 'Python 3.12+ is required on PATH (python or python3).' }
$hostArchitecture = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
    'X64' { 'x86_64' }
    'Arm64' { 'aarch64' }
    default { throw "Unsupported architecture: $_" }
}
if (-not $Architecture) { $Architecture = $hostArchitecture }
if ($Install -and $hostArchitecture -eq 'x86_64' -and $Architecture -eq 'aarch64') {
    throw 'An ARM64 installer cannot run on x86-64 Windows. Omit -Install to cross-build it.'
}

$root = Split-Path -Parent $PSScriptRoot
$target = "$Architecture-pc-windows-msvc"
$previousOffline = $env:CARGO_NET_OFFLINE
$previousFxc = $env:GPUI_FXC_PATH
Push-Location $root
try {
    if ($Offline) { $env:CARGO_NET_OFFLINE = 'true' }
    $vsDevShell = Resolve-VsDevShell $Architecture
    Write-Host "VS developer shell: $vsDevShell"
    Initialize-VsDevShell $vsDevShell $Architecture $hostArchitecture
    $null = Get-Command link.exe -CommandType Application -ErrorAction Stop
    $resourceCompiler = if ($env:RC) { $env:RC } else { 'rc.exe' }
    $null = Get-Command $resourceCompiler -CommandType Application -ErrorAction Stop
    if (-not $env:GPUI_FXC_PATH) {
        $compiler = Get-Command fxc.exe -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if (-not $compiler) { throw 'fxc.exe was not found. Install the Windows SDK or set GPUI_FXC_PATH to its fxc.exe.' }
        $env:GPUI_FXC_PATH = $compiler.Source
    }
    if (-not (Test-Path -LiteralPath $env:GPUI_FXC_PATH -PathType Leaf)) {
        throw 'GPUI_FXC_PATH must name the Windows SDK shader compiler.'
    }
    # Pin an absolute path after VS setup; GPUI's fallback can find several.
    $env:GPUI_FXC_PATH = (Resolve-Path -LiteralPath $env:GPUI_FXC_PATH).ProviderPath
    Write-Host "Shader compiler: $env:GPUI_FXC_PATH"

    $installedTargets = & rustup target list --installed
    if ($LASTEXITCODE -ne 0) { throw 'Could not query installed Rust targets.' }
    if ($installedTargets -notcontains $target) {
        if ($env:CARGO_NET_OFFLINE -eq 'true') {
            throw "Rust target $target is not installed. Before using -Offline, run: rustup target add $target"
        }
        & rustup target add $target
        if ($LASTEXITCODE -ne 0) { throw "Could not install Rust target $target" }
    }

    # Licenses need the locked graph for all platforms.
    $metadataJson = & cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Cargo metadata or fetch the locked dependencies.' }
    $metadata = ($metadataJson -join "`n") | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq 'desktop' } | Select-Object -First 1
    $version = $package.version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Installer requires a numeric desktop package version.' }
    if (-not ($package.targets | Where-Object { $_.name -eq 'subroutine-lite' -and $_.kind -contains 'bin' })) {
        throw 'Missing subroutine-lite binary target.'
    }
    $targetDir = $metadata.target_directory
    $buildDir = Join-Path $targetDir "$target/release"
    $outputDir = Join-Path $buildDir 'bundle/windows'
    $resources = Join-Path $root 'crates/desktop/resources/windows'

    & cargo build --locked --release --package desktop --bin subroutine-lite --target $target --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { throw 'Subroutine Lite build failed.' }
    $exe = Join-Path $buildDir 'subroutine-lite.exe'
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($exe)
    if ($info.ProductName -ne 'Subroutine Lite' -or $info.FileDescription -ne 'Subroutine Lite' -or
        $info.OriginalFilename -ne 'subroutine-lite.exe' -or $info.ProductVersion -ne $version) {
        throw 'Executable version resource does not identify this Subroutine Lite release.'
    }

    $staging = Join-Path ([IO.Path]::GetTempPath()) ("subroutine-lite-" + [Guid]::NewGuid())
    $null = New-Item -ItemType Directory -Path (Join-Path $staging 'notices')
    try {
        Copy-Item -LiteralPath $exe -Destination $staging
        Copy-Item -LiteralPath (Join-Path $resources 'Subroutine.ico') -Destination $staging
        & $python (Join-Path $root 'crates/desktop/resources/packaging/licenses.py') (Join-Path $staging 'notices') --target $target --release
        if ($LASTEXITCODE -ne 0) { throw 'License generation failed.' }
        & $inno "/DResourcesDir=$staging" "/DOutputDir=$staging" "/DVersion=$version" "/DArchitecture=$Architecture" (Join-Path $resources 'subroutine-lite.iss')
        if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }
        $filename = "Subroutine-Lite-$version-$Architecture-setup.exe"
        $installer = Join-Path $outputDir $filename
        $stagedInstaller = Join-Path $staging $filename
        if (-not (Test-Path -LiteralPath $stagedInstaller -PathType Leaf) -or (Get-Item -LiteralPath $stagedInstaller).Length -eq 0) {
            throw 'Inno Setup did not produce an installer.'
        }
        $null = New-Item -ItemType Directory -Force -Path $outputDir
        Copy-Item -LiteralPath $stagedInstaller -Destination $installer -Force
        Write-Output "Built unsigned installer: $installer"
    } finally {
        Remove-Item -LiteralPath $staging -Recurse -Force
    }

    if ($Install) {
        Assert-AppStopped
        $installLog = Join-Path $outputDir 'Subroutine-Lite-install.log'
        $installArgs = @('/SILENT', '/NOCLOSEAPPLICATIONS', '/NORESTART', '/LaunchAfterInstall=1', "/LOG=`"$installLog`"")
        $installation = Start-Process -FilePath $installer -ArgumentList $installArgs -PassThru
        # Wait for the installer only; Inno launches the app with nowait.
        $null = $installation.Handle
        $installation.WaitForExit()
        if ($installation.ExitCode -ne 0) {
            throw "Subroutine Lite installation failed or was cancelled (exit $($installation.ExitCode)). See $installLog"
        }
        Write-Output "Installed/updated; launch requested. Log: $installLog"
    }
} finally {
    $env:CARGO_NET_OFFLINE = $previousOffline
    $env:GPUI_FXC_PATH = $previousFxc
    Pop-Location
}
