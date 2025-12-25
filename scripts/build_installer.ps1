param(
    [string]$OutDir = "dist/windows",
    [string]$ModelsDir = "$env:APPDATA\\.sempal\\models",
    [switch]$Sign
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$BundleDir = Join-Path $OutDir "bundle"
$ModelsOut = Join-Path $BundleDir "models"
$BurnpackName = "panns_cnn14_16k.bpk"

Write-Host "Building release binaries..."
cargo build --release

if (Test-Path $OutDir) {
    Remove-Item -Recurse -Force $OutDir
}

New-Item -ItemType Directory -Force -Path $BundleDir | Out-Null
New-Item -ItemType Directory -Force -Path $ModelsOut | Out-Null

Copy-Item "target/release/sempal.exe" $BundleDir -Force
Copy-Item "target/release/sempal-installer.exe" $OutDir -Force
Copy-Item "assets/logo3.ico" (Join-Path $BundleDir "sempal.ico") -Force

$BurnpackCandidate = Get-ChildItem -Path "target/release/build" -Recurse -Filter $BurnpackName -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $BurnpackCandidate -and (Test-Path (Join-Path $ModelsDir $BurnpackName))) {
    $BurnpackCandidate = Get-Item (Join-Path $ModelsDir $BurnpackName)
}
if ($BurnpackCandidate) {
    Copy-Item $BurnpackCandidate.FullName $ModelsOut -Force
} else {
    throw "$BurnpackName not found in target/release/build or $ModelsDir"
}

Copy-Item "build/windows/installer_manifest.json" $OutDir -Force

if ($Sign) {
    if (-not $env:SIGNTOOL_PATH -or -not $env:SIGN_CERT_PATH) {
        Write-Warning "SIGNTOOL_PATH and SIGN_CERT_PATH must be set to sign binaries."
    } else {
        & $env:SIGNTOOL_PATH sign /fd SHA256 /f $env:SIGN_CERT_PATH /tr http://timestamp.digicert.com "$OutDir\\sempal-installer.exe"
        & $env:SIGNTOOL_PATH sign /fd SHA256 /f $env:SIGN_CERT_PATH /tr http://timestamp.digicert.com "$BundleDir\\sempal.exe"
    }
}

Write-Host "Bundle ready at $OutDir"
