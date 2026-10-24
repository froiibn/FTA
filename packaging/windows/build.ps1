# Build on the target OS: macOS checks cannot prove Windows linking or installer generation.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ExpectedHost = 'x86_64-pc-windows-msvc'
$ExpectedNodeMajor = 24
$Workspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$RunId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
$Output = Join-Path $Workspace "output\windows-x64\$RunId"

function Invoke-Checked {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $File $($Arguments -join ' ')"
    }
}

# Reject foreign output before packaging; a same-named executable is not proof of x64 machine code.
function Assert-X64Executable {
    param([string]$Path)
    $Stream = [IO.File]::OpenRead($Path)
    $Reader = [IO.BinaryReader]::new($Stream)
    try {
        if ($Reader.ReadUInt16() -ne 0x5A4D) { throw "Not a PE executable: $Path" }
        $Stream.Position = 0x3C
        $PeOffset = $Reader.ReadUInt32()
        $Stream.Position = $PeOffset
        if ($Reader.ReadUInt32() -ne 0x00004550 -or $Reader.ReadUInt16() -ne 0x8664) {
            throw "Expected x64 PE executable: $Path"
        }
    } finally {
        $Reader.Dispose()
        $Stream.Dispose()
    }
}

if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitOperatingSystem) {
    throw 'Run this script on Windows x64 with the MSVC Rust toolchain.'
}
foreach ($Tool in @('cargo', 'rustc', 'node', 'npm.cmd')) {
    if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) { throw "Required tool not found: $Tool" }
}
# A target override changes Cargo output layout and would diverge from the native sidecar wrapper.
if ($env:CARGO_BUILD_TARGET) { throw 'Unset CARGO_BUILD_TARGET; this script uses the native MSVC host.' }
$VsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $VsWhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK.' }
$VsInstallation = & $VsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $VsInstallation) { throw 'The Visual Studio MSVC x64/x86 build tools component is missing.' }

# Every run has a new evidence directory, so failed builds cannot masquerade as previous successful installers.
New-Item -ItemType Directory -Path $Output -ErrorAction Stop | Out-Null
Start-Transcript -Path (Join-Path $Output 'build.log') | Out-Null
$OriginalLocation = Get-Location
try {
    Set-Location $Workspace
    $RustVersion = & rustc -vV
    if ($LASTEXITCODE -ne 0) { throw 'Unable to load the pinned Rust toolchain.' }
    if (-not ($RustVersion -contains "host: $ExpectedHost")) { throw "Rust host must be $ExpectedHost" }
    # Windows PowerShell 5 removes embedded native-argument quotes; use quote-free expressions.
    $NodeVersion = & node --version
    if ($LASTEXITCODE -ne 0) { throw 'Unable to query Node.js version.' }
    $NodeArch = & node -p process.arch
    if ($LASTEXITCODE -ne 0 -or $NodeVersion -notmatch "^v$ExpectedNodeMajor\." -or $NodeArch -ne 'x64') {
        throw "Install Node.js $ExpectedNodeMajor x64 before building."
    }
    Invoke-Checked cargo @('fmt', '--all', '--check')
    Invoke-Checked cargo @('test', '--locked')
    Invoke-Checked cargo @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')
    # Fresh source archives do not contain dist; prepare assets before the desktop Rust checks.
    Invoke-Checked npm.cmd @('--prefix', 'apps/desktop', 'ci')
    Invoke-Checked npm.cmd @('--prefix', 'apps/desktop', 'run', 'build')
    Invoke-Checked cargo @('check', '--locked', '-p', 'fta-desktop')
    Invoke-Checked cargo @('test', '--locked', '-p', 'fta-desktop')
    Invoke-Checked cargo @('clippy', '--locked', '-p', 'fta-desktop', '--all-targets', '--', '-D', 'warnings')
    Invoke-Checked cargo @('build', '--locked', '--release', '-p', 'fta-cli')
    $PackageStarted = [DateTime]::UtcNow
    Invoke-Checked npm.cmd @('--prefix', 'apps/desktop', 'run', 'bundle', '--', '--bundles', 'nsis')

    # Respect Cargo target_directory instead of assuming a drive, user folder or target path.
    $MetadataJson = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'Unable to resolve Cargo artifact directory.' }
    $Metadata = $MetadataJson | ConvertFrom-Json
    $Release = Join-Path $Metadata.target_directory 'release'
    foreach ($Name in @('fta', 'fta-engine', 'fta-mcp', 'fta-desktop')) {
        $Executable = Join-Path $Release "$Name.exe"
        Assert-X64Executable $Executable
        if ($Name -ne 'fta-desktop') { Copy-Item -LiteralPath $Executable -Destination $Output }
    }
    $Installers = @(Get-ChildItem -LiteralPath (Join-Path $Release 'bundle\nsis') -Filter '*-setup.exe' -File |
        Where-Object { $_.LastWriteTimeUtc -ge $PackageStarted.AddSeconds(-2) })
    if ($Installers.Count -ne 1) { throw 'Expected exactly one newly built NSIS installer.' }
    # NSIS itself may use a 32-bit loader even when all installed app binaries are x64.
    Copy-Item -LiteralPath $Installers[0].FullName -Destination $Output
    $Config = Get-Content (Join-Path $Workspace 'apps\desktop\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
    [ordered]@{
        product = $Config.productName
        version = $Config.version
        host = $ExpectedHost
        built_at_utc = [DateTime]::UtcNow.ToString('o')
        rust = $RustVersion
        node = (& node --version)
        windows = [Environment]::OSVersion.VersionString
        installer = $Installers[0].Name
        checks = 'fmt, headless test/clippy, desktop check/test/clippy, TypeScript/Vite build'
        runtime_verification = 'Not performed by this build script; follow packaging/windows/README.md.'
    } | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Output 'build-info.json')
    Get-ChildItem -LiteralPath $Output -File | Where-Object { $_.Extension -eq '.exe' } |
        Sort-Object Name | ForEach-Object {
            $Hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256
            '{0}  {1}' -f $Hash.Hash.ToLowerInvariant(), $_.Name
        } | Set-Content -Encoding ASCII (Join-Path $Output 'SHA256SUMS.txt')
    Write-Host "Windows x64 build complete: $Output"
} finally {
    Set-Location $OriginalLocation
    Stop-Transcript | Out-Null
}
