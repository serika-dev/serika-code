# Development environment for the Qt desktop (qt/serika-code).
#
#   . .\scripts\qt-env.ps1
#   Build-SerikaQt                       # configure (first time) and build
#   Shoot-SerikaQt app app.png           # demo screenshot: login | welcome | app, optional popup
#
# Uses the Qt 6.8 LLVM MinGW kit from the Qt installer (default C:\Users\<you>\Qt).

$qtRoot = $env:SERIKA_QT_ROOT
if (-not $qtRoot) { $qtRoot = Join-Path $env:USERPROFILE "Qt" }
$qtKit = Get-ChildItem -Path $qtRoot -Directory -Filter "6.*" -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending | Select-Object -First 1 |
    ForEach-Object { Join-Path $_.FullName "llvm-mingw_64" }
if (-not $qtKit -or -not (Test-Path $qtKit)) { throw "Qt 6 llvm-mingw kit not found under '$qtRoot'. Set SERIKA_QT_ROOT." }

$toolsDir = Join-Path $qtRoot "Tools"
$mingw = Get-ChildItem -Path $toolsDir -Directory -Filter "llvm-mingw*" | Select-Object -First 1
$cmake = Join-Path $toolsDir "CMake_64\bin"
$ninja = Join-Path $toolsDir "Ninja"
$env:PATH = (@($cmake, $ninja, (Join-Path $mingw.FullName "bin"), (Join-Path $qtKit "bin")) -join ';') + ';' + $env:PATH
$env:QT_ASSUME_STDERR_HAS_CONSOLE = "1"

$script:QtRepo = Split-Path -Parent $PSScriptRoot
$script:QtSource = Join-Path $script:QtRepo "qt\serika-code"
$script:QtBuild = Join-Path $script:QtSource "build"
$script:QtKit = $qtKit
$script:QtMingw = $mingw.FullName

function Build-SerikaQt {
    if (-not (Test-Path (Join-Path $script:QtBuild "build.ninja"))) {
        cmake -S $script:QtSource -B $script:QtBuild -G Ninja -DCMAKE_BUILD_TYPE=Release `
            "-DCMAKE_PREFIX_PATH=$script:QtKit" `
            "-DCMAKE_CXX_COMPILER=$(Join-Path $script:QtMingw 'bin\clang++.exe')" | Out-Null
    }
    cmake --build $script:QtBuild
}

function Shoot-SerikaQt {
    param([string]$Page = "app", [string]$Out = "target\ui-shots\qt.png", [string]$Popup = "", [string]$Theme = "",
          [string]$Preset = "", [string]$Tab = "", [string]$Panel = "")
    $args = @("--demo", "--page", $Page, "--shot", $Out)
    if ($Popup) { $args += @("--popup", $Popup) }
    if ($Theme) { $args += @("--theme", $Theme) }
    if ($Preset) { $args += @("--preset", $Preset) }
    if ($Tab) { $args += @("--tab", $Tab) }
    if ($Panel) { $args += @("--panel", $Panel) }
    New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
    Remove-Item $Out -ErrorAction SilentlyContinue
    $err = "$Out.err"
    $proc = Start-Process -FilePath (Join-Path $script:QtBuild "serika-code.exe") -ArgumentList $args `
        -WorkingDirectory $script:QtRepo -PassThru -Wait -NoNewWindow -RedirectStandardError $err
    if (Test-Path $err) { Get-Content $err | Select-Object -First 25 }
    "exit=$($proc.ExitCode) png=$(Test-Path $Out)"
}
