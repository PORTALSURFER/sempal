$ErrorActionPreference = "Stop"

$rootDir = Split-Path -Parent $PSScriptRoot
$outPath = Join-Path $rootDir "tests\golden_embedding.json"

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outPath) | Out-Null
python "$rootDir\tools\generate_panns_golden_embedding.py" --out $outPath
Write-Host "Wrote $outPath"
