param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [switch]$Push
)
# Bump workspace version in Cargo.toml, commit, and create annotated git tag v<Version>
$clean = $Version.TrimStart('v')
if ($clean -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "Version must be semantic (e.g. 0.1.0 or 0.2.0-beta.1)"
}

$cargoPath = Join-Path $PSScriptRoot "..\Cargo.toml"
$content = Get-Content $cargoPath -Raw
$updated = $content -replace '(?m)^version\s*=\s*"[^"]+"', "version = `"$clean`""
Set-Content -Path $cargoPath -Value $updated -NoNewline

Write-Host "Updated Cargo.toml workspace version to $clean" -ForegroundColor Green
git add Cargo.toml
git commit -m "chore(release): v$clean"
git tag -a "v$clean" -m "Serika Code v$clean"
if ($Push) {
    git push origin main --follow-tags
}
