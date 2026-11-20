# Build LarvaPHP with the MSVC environment automatically configured.
#
# Usage:
#   .\build.ps1                          # debug build
#   .\build.ps1 --release                # release build
#   .\build.ps1 --features embedded      # with HTTP/3/embedded features
#   .\build.ps1 test --lib               # run tests
#
# It locates Visual Studio via vswhere, imports the Developer PowerShell module,
# and then runs `cargo` with whatever arguments you pass.

$ErrorActionPreference = "Stop"

# 1. Locate Visual Studio with the C++ toolchain.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    Write-Host "vswhere.exe not found. Is Visual Studio installed?" -ForegroundColor Red
    exit 1
}

$vsPath = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath

if (-not $vsPath) {
    Write-Host "Visual Studio with C++ tools not found." -ForegroundColor Red
    exit 1
}

Write-Host "Using Visual Studio: $vsPath" -ForegroundColor Cyan

# 2. Import the Developer PowerShell module and enter the x64 dev shell.
$devShell = Join-Path $vsPath "Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
if (Test-Path $devShell) {
    Import-Module $devShell
    Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation `
        -DevCmdArguments "-arch=x64 -host_arch=x64" | Out-Null
} else {
    # Fallback: set the environment variables manually via vcvarsall.
    $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
    if (Test-Path $vcvars) {
        cmd /c "`"$vcvars`" && set" | ForEach-Object {
            if ($_ -match "=") {
                $i = $_.IndexOf("=")
                [System.Environment]::SetEnvironmentVariable($_.Substring(0, $i), $_.Substring($i + 1))
            }
        }
    }
}

# 3. Run cargo with the caller's arguments.
& cargo @args
exit $LASTEXITCODE