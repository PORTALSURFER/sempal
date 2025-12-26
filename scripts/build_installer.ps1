param(
    [string]$OutDir = "dist/windows",
    [switch]$Sign
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$BundleDir = Join-Path $OutDir "bundle"

Write-Host "Building release binaries..."
if (-not $env:SEMPAL_PANNS_ONNX_PATH) {
    $onnxDir = Join-Path $RepoRoot ".tmp\\panns_onnx"
    $onnxPath = Join-Path $onnxDir "panns_cnn14_16k.onnx"
    if (-not (Test-Path $onnxPath)) {
        Write-Host "Generating PANNs ONNX from checkpoint..."
        python "tools\\export_panns_onnx.py" --out-dir $onnxDir
    }
    $env:SEMPAL_PANNS_ONNX_PATH = $onnxPath
}
cargo build --release

if (Test-Path $OutDir) {
    Remove-Item -Recurse -Force $OutDir
}

New-Item -ItemType Directory -Force -Path $BundleDir | Out-Null
Copy-Item "target/release/sempal.exe" $BundleDir -Force
Copy-Item "target/release/sempal-installer.exe" $BundleDir -Force
Copy-Item "target/release/sempal-installer.exe" $OutDir -Force
Copy-Item "assets/logo3.ico" (Join-Path $BundleDir "sempal.ico") -Force
$modelsDir = Join-Path $BundleDir "models"
New-Item -ItemType Directory -Force -Path $modelsDir | Out-Null
$burnpack = Get-ChildItem -Path "target/release/build" -Recurse -Filter "panns_cnn14_16k.bpk" | Select-Object -First 1
if (-not $burnpack) { throw "Burnpack not found in target/release/build; ensure the ONNX model is available for the build." }
Copy-Item $burnpack.FullName $modelsDir -Force

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
