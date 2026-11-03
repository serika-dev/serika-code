# Build the Qt desktop and bundle it with the Qt libraries it needs.
#
#   .\scripts\package-qt.ps1            # produces dist\serika-code\
#   .\scripts\package-qt.ps1 -Install   # also runs `serika setup` (installs to %LOCALAPPDATA%\Programs\Serika Code)
#
# `serika setup` copies dist\serika-code into <install>\app and points the Start Menu and
# Desktop shortcuts at it. The folder carries its own serika.exe, so it runs on its own.
param([switch]$Install)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# Rust first: the Qt environment puts LLVM MinGW on PATH, which rustc must not see.
. (Join-Path $PSScriptRoot "dev-env.ps1") | Out-Null
$ErrorActionPreference = "Continue"
cargo build --release -p serika-cli
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "qt-env.ps1")
Build-SerikaQt
if ($LASTEXITCODE -ne 0) { throw "Qt build failed" }

$dist = Join-Path $repo "dist\serika-code"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $script:QtBuild "serika-code.exe") $dist
Copy-Item (Join-Path $repo "target\release\serika.exe") $dist

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

$size = (Get-ChildItem $dist -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
"packaged $dist ({0:N0} MB)" -f $size

if ($Install) {
    & (Join-Path $repo "target\release\serika.exe") setup
}
