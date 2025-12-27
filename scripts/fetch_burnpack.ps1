param(
    [string]$Dest = "assets\\ml\\panns_cnn14_16k\\panns_cnn14_16k.bpk",
    [string]$Url = $env:SEMPAL_PANNS_BURNPACK_URL,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

if (-not $Url -or $Url.Trim().Length -eq 0) {
    $Url = "https://github.com/PORTALSURFER/sempal/releases/download/core-files/panns_cnn14_16k.bpk"
}

if ((Test-Path $Dest) -and -not $Force) {
    Write-Host "Burnpack already present at $Dest"
    exit 0
}

$destDir = Split-Path -Parent $Dest
if ($destDir -and -not (Test-Path $destDir)) {
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
}

Invoke-WebRequest -Uri $Url -OutFile $Dest

if (-not (Test-Path $Dest)) {
    throw "Burnpack download failed: $Dest not found."
}
