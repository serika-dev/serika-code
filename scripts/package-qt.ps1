# Build the Qt desktop and bundle it with the Qt libraries it needs.
#
#   .\scripts\package-qt.ps1              # produces dist\serika-code\ and dist\serika-code-<ver>-windows-x64.zip
#   .\scripts\package-qt.ps1 -Installer   # also dist\serika-code-<ver>-windows-x64-setup.exe (needs Inno Setup 6)
#   .\scripts\package-qt.ps1 -Install     # also runs `serika setup` (installs to %LOCALAPPDATA%\Programs\Serika Code)
#   .\scripts\package-qt.ps1 -Cli path\to\serika.exe   # bundle a CLI built elsewhere (CI) instead of building it
#
# `serika setup` copies dist\serika-code into <install>\app and points the Start Menu and
# Desktop shortcuts at it. The folder carries its own serika.exe, so it runs on its own.
param([switch]$Install, [switch]$Installer, [string]$Cli = "")

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$version = (Select-String -Path (Join-Path $repo "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value

if (-not $Cli) {
    # Rust first: the Qt environment puts LLVM MinGW on PATH, which rustc must not see.
    . (Join-Path $PSScriptRoot "dev-env.ps1") | Out-Null
    $ErrorActionPreference = "Continue"
    cargo build --release -p serika-cli
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $ErrorActionPreference = "Stop"
    $Cli = Join-Path $repo "target\release\serika.exe"
}

. (Join-Path $PSScriptRoot "qt-env.ps1")
Build-SerikaQt
if ($LASTEXITCODE -ne 0) { throw "Qt build failed" }

$dist = Join-Path $repo "dist\serika-code"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $script:QtBuild "serika-code.exe") $dist
Copy-Item $Cli (Join-Path $dist "serika.exe")

# No --release: windeployqt reads the LLVM MinGW Qt DLLs as debug builds and then finds no
# platform plugin. The kit only ships one flavour, so there is nothing to choose between.
# Its warnings go to stderr, which must not stop the script.
$ErrorActionPreference = "Continue"
& (Join-Path $script:QtKit "bin\windeployqt.exe") --no-translations --no-system-d3d-compiler `
    --qmldir (Join-Path $script:QtSource "qml") (Join-Path $dist "serika-code.exe") 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "windeployqt failed" }
$ErrorActionPreference = "Stop"
# The LLVM MinGW C++ runtime, which windeployqt only copies for GCC MinGW kits.
foreach ($dll in "libc++.dll", "libunwind.dll") {
    $from = Join-Path $script:QtKit "bin\$dll"
    if (Test-Path $from) { Copy-Item $from $dist }
}
# Debugger plugins are only for development, and the app draws every control in the Basic
# style (main.cpp), so the other Quick Controls styles never load.
Remove-Item (Join-Path $dist "qmltooling") -Recurse -Force -ErrorAction SilentlyContinue
foreach ($style in "FluentWinUI3", "Fusion", "Imagine", "Material", "Universal", "Windows") {
    Remove-Item (Join-Path $dist "qml\QtQuick\Controls\$style") -Recurse -Force -ErrorAction SilentlyContinue
    Get-ChildItem $dist -Filter "Qt6QuickControls2$style*.dll" | Remove-Item -Force
}
Remove-Item (Join-Path $dist "qml\QtQuick\NativeStyle") -Recurse -Force -ErrorAction SilentlyContinue

$size = (Get-ChildItem $dist -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
"packaged $dist ({0:N0} MB)" -f $size

function Write-Checksum([string]$file) {
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
    "$hash  $(Split-Path -Leaf $file)" | Out-File -FilePath "$file.sha256" -Encoding ascii -NoNewline
}

$zip = Join-Path $repo "dist\serika-code-$version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $dist -DestinationPath $zip -CompressionLevel Optimal
Write-Checksum $zip
"zipped $zip"

if ($Installer) {
    $iscc = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) { throw "Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup" }
    $art = Join-Path $repo "dist\installer-art"
    python -I (Join-Path $PSScriptRoot "make_installer_art.py") $art | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "installer pictures failed" }
    & $iscc /Q "/DAppVersion=$version" "/DSourceDir=$dist" "/DArtDir=$art" "/DOutputDir=$(Join-Path $repo 'dist')" `
        (Join-Path $repo "packaging\windows\serika-code.iss")
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    $setup = Join-Path $repo "dist\serika-code-$version-windows-x64-setup.exe"
    Write-Checksum $setup
    "installer $setup ({0:N1} MB)" -f ((Get-Item $setup).Length / 1MB)
}

if ($Install) {
    & (Join-Path $dist "serika.exe") setup
}
