# Development environment for the Rust GNU toolchain on Windows.
#
# Dot-source it in a fresh PowerShell before building:
#
#   . .\scripts\dev-env.ps1
#   cargo test --workspace
#
# Why this exists:
#
# * Rust's x86_64-pc-windows-gnu target links with its bundled ("self-contained")
#   gcc + GNU ld and the bundled libgcc / msvcrt import libraries. That bundled gcc
#   has no cc1, so it cannot compile C (bundled SQLite in libsqlite3-sys).
# * LLVM MinGW can compile C, but if its x86_64-w64-mingw32-gcc ends up on PATH,
#   rustc uses it as the linker and the link fails (`-lgcc`, `-lgcc_eh`), because
#   LLVM MinGW uses compiler-rt and UCRT instead of libgcc and msvcrt.
# * Rust's bundled GNU dlltool needs an `as` assembler, which is not in the Rust
#   installer. Using LLVM MinGW's llvm-dlltool directly produces short-import COFF
#   archives, and mixing those with the bundled full-idata archives under GNU ld
#   corrupts the PE Import Address Table when the same DLL is imported twice
#   (e.g. dwmapi.dll from both std and raw-window-handle). Wrapping the bundled
#   GNU dlltool and pointing its `-S` flag at LLVM MinGW's clang assembler keeps
#   every import library in traditional GNU idata format.
#
# Note: do not set $ErrorActionPreference = "Stop" here. This file is
# dot-sourced, and that setting would turn cargo's normal stderr progress output
# into terminating errors for the rest of the shell session.

$rustRoot = $env:SERIKA_RUST_ROOT
if (-not $rustRoot) { $rustRoot = "C:\Program Files\Rust stable GNU 1.99" }
$rustBin = Join-Path $rustRoot "bin"
$rustSelfContained = Join-Path $rustRoot "lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained"
$gnuDlltool = Join-Path $rustSelfContained "dlltool.exe"

$mingw = $env:SERIKA_LLVM_MINGW_BIN
if (-not $mingw) {
    $pkgRoot = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages"
    $pkg = Get-ChildItem -Path $pkgRoot -Directory -Filter "MartinStorsjo.LLVM-MinGW.UCRT*" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($pkg) {
        $inner = Get-ChildItem -Path $pkg.FullName -Directory -Filter "llvm-mingw-*" | Select-Object -First 1
        if ($inner) { $mingw = Join-Path $inner.FullName "bin" }
    }
}

if (-not (Test-Path (Join-Path $rustBin "cargo.exe"))) {
    throw "Rust GNU toolchain not found at '$rustBin'. Set SERIKA_RUST_ROOT."
}
if (-not (Test-Path $gnuDlltool)) {
    throw "Bundled GNU dlltool not found at '$gnuDlltool'."
}
if (-not $mingw -or -not (Test-Path (Join-Path $mingw "x86_64-w64-mingw32-clang.exe"))) {
    throw "LLVM MinGW not found. Install it (winget install MartinStorsjo.LLVM-MinGW.UCRT) or set SERIKA_LLVM_MINGW_BIN."
}

$clang = Join-Path $mingw "x86_64-w64-mingw32-clang.exe"
$ar = Join-Path $mingw "llvm-ar.exe"
$as = Join-Path $mingw "x86_64-w64-mingw32-as.exe"

# Build a tiny native shim for GNU dlltool + clang assembler if not already built.
$shimDir = Join-Path ([System.IO.Path]::GetTempPath()) "serika-gnu-toolchain"
New-Item -ItemType Directory -Force -Path $shimDir | Out-Null
$shimExe = Join-Path $shimDir "serika-dlltool.exe"
$shimSrc = Join-Path $shimDir "serika-dlltool.rs"

$expectedSrc = @"
use std::process::{Command, exit};
fn main() {
    let mut cmd = Command::new(r"$gnuDlltool");
    cmd.arg("-S").arg(r"$as").arg("-f").arg("-c");
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "-f" || arg == "--as-flags" {
            let _ = args.next();
            continue;
        }
        cmd.arg(arg);
    }
    match cmd.status() {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => { eprintln!("serika-dlltool: {e}"); exit(1); }
    }
}
"@

$needCompile = -not (Test-Path $shimExe)
if (-not $needCompile -and (Test-Path $shimSrc)) {
    $current = [System.IO.File]::ReadAllText($shimSrc)
    if ($current -ne $expectedSrc) { $needCompile = $true }
} else {
    $needCompile = $true
}

# Remove any LLVM MinGW / other gcc directories from PATH so rustc does not pick
# them up as the linker, then make sure the Rust toolchain is first.
$parts = $env:PATH -split ';' | Where-Object {
    $_ -and ($_ -ne $rustBin) -and -not (Test-Path (Join-Path $_ "x86_64-w64-mingw32-gcc.exe")) -and -not (Test-Path (Join-Path $_ "gcc.exe"))
}
$env:PATH = (@($rustBin) + $parts) -join ';'

if ($needCompile) {
    [System.IO.File]::WriteAllText($shimSrc, $expectedSrc)
    & (Join-Path $rustBin "rustc.exe") --edition 2021 -O $shimSrc -o $shimExe
    if ($LASTEXITCODE -ne 0) { throw "Failed to compile dlltool shim" }
}

$env:CC_x86_64_pc_windows_gnu = $clang
$env:AR_x86_64_pc_windows_gnu = $ar
$env:CFLAGS_x86_64_pc_windows_gnu = "-U_UCRT -D__MSVCRT_VERSION__=0x700"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-C dlltool=$shimExe"

Remove-Item Env:CC -ErrorAction SilentlyContinue
Remove-Item Env:AR -ErrorAction SilentlyContinue

Write-Host "rust     : $rustBin"
Write-Host "CC       : $env:CC_x86_64_pc_windows_gnu $env:CFLAGS_x86_64_pc_windows_gnu"
Write-Host "AR       : $env:AR_x86_64_pc_windows_gnu"
Write-Host "dlltool  : $shimExe"
