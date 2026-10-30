param(
    [switch]$FromSource,
    [string]$Prefix = "",
    [string]$Version = "latest"
)
# Serika Code installer for Windows
# Usage:
#   irm https://raw.githubusercontent.com/serika-dev/serika-code/main/install.ps1 | iex
#   powershell -ExecutionPolicy Bypass -File .\install.ps1

$Repo = if ($env:SERIKA_REPO) { $env:SERIKA_REPO } else { "serika-dev/serika-code" }
$Target = "x86_64-pc-windows-msvc"
$Asset = "serika-code-$Target.zip"

function Install-FromDir([string]$SourceDir) {
    $serikaExe = Join-Path $SourceDir "serika.exe"
    if (-not (Test-Path $serikaExe)) {
        throw "serika.exe not found in $SourceDir"
    }
    if ($Prefix) {
        & $serikaExe setup --prefix $Prefix
    } else {
        & $serikaExe setup
    }
}

$localRelease = Join-Path $PSScriptRoot "target\release\serika.exe"
if (-not $FromSource -and (Test-Path $localRelease)) {
    Write-Host "==> Installing Serika Code from local release build..." -ForegroundColor Cyan
    Install-FromDir (Split-Path -Parent $localRelease)
    return
}

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("serika-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
    if (-not $FromSource) {
        $baseUrl = if ($Version -eq "latest") {
            "https://github.com/$Repo/releases/latest/download"
        } else {
            "https://github.com/$Repo/releases/download/$Version"
        }
        $zipPath = Join-Path $tmpDir $Asset
        $headers = @{}
        $token = if ($env:SERIKA_GITHUB_TOKEN) { $env:SERIKA_GITHUB_TOKEN } else { $env:GITHUB_TOKEN }
        if ($token) {
            $headers["Authorization"] = "Bearer $token"
        }
        try {
            Write-Host "==> Downloading $Asset from $Repo ($Version)..." -ForegroundColor Cyan
            Invoke-WebRequest -Uri "$baseUrl/$Asset" -OutFile $zipPath -Headers $headers -UseBasicParsing -ErrorAction Stop
            $extractDir = Join-Path $tmpDir "extracted"
            Expand-Archive -LiteralPath $zipPath -DestinationPath $extractDir -Force
            $found = Get-ChildItem -Path $extractDir -Filter "serika.exe" -Recurse | Select-Object -First 1
            if ($found) {
                Install-FromDir $found.DirectoryName
                return
            }
        } catch {
            Write-Host "==> Prebuilt release download unavailable; building from source..." -ForegroundColor Yellow
        }
    }

    if (Test-Path (Join-Path $PSScriptRoot "Cargo.toml")) {
        Push-Location $PSScriptRoot
        try {
            if (Test-Path ".\scripts\dev-env.ps1") {
                . .\scripts\dev-env.ps1 | Out-Null
            }
            cmd /c "cargo build --release --workspace"
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
            Install-FromDir (Join-Path $PSScriptRoot "target\release")
        } finally {
            Pop-Location
        }
    } else {
        throw "Could not download release archive and no local Cargo workspace found."
    }
} finally {
    if (Test-Path $tmpDir) {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}
