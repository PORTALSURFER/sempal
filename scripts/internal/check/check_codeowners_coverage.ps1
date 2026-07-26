Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
Ensures `.github/CODEOWNERS` covers the high-level ownership buckets.

.DESCRIPTION
This is intentionally lightweight: it checks for coverage, not exact matches.
Keep `docs/TARGET.md` (ownership map) and `.github/CODEOWNERS` (enforcement) in sync.
#>

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $rootDir
try {
  $codeownersPath = ".github/CODEOWNERS"
  if (-not (Test-Path -LiteralPath $codeownersPath -PathType Leaf)) {
    throw "[codeowners_coverage] Missing $codeownersPath"
  }

  $patterns = New-Object "System.Collections.Generic.List[string]"
  foreach ($line in Get-Content -LiteralPath $codeownersPath) {
    $t = $line.Trim()
    if ([string]::IsNullOrWhiteSpace($t)) { continue }
    if ($t.StartsWith("#")) { continue }
    $parts = $t -split '\s+'
    if ($parts.Length -gt 0) {
      $patterns.Add($parts[0])
    }
  }

  function Has-Prefix([string]$Prefix) {
    foreach ($p in $patterns) {
      if ($p -eq $Prefix) { return $true }
      if ($p.StartsWith($Prefix)) { return $true }
    }
    return $false
  }

  $required = @(
    "*",
    "/.github/",
    "/scripts/",
    "/docs/",
    "/manual/",
    "/apps/",
    "/tools/",
    "/src/app_core/",
    "/src/app/",
    "/src/analysis/",
    "/src/audio/",
    "/src/gui/",
    "/src/gui_runtime/",
    "/src/gui_test/",
    "/src/sample_sources/",
    "/src/selection/",
  )

  $missing = @()
  foreach ($p in $required) {
    if (-not (Has-Prefix $p)) { $missing += $p }
  }

  if ($missing.Count -gt 0) {
    Write-Error "[codeowners_coverage] Missing required CODEOWNERS bucket entries:"
    Write-Host "[codeowners_coverage] (Update docs/TARGET.md and .github/CODEOWNERS together.)"
    foreach ($m in $missing) {
      Write-Host (" - {0}" -f $m)
    }
    exit 1
  }

  Write-Host "[codeowners_coverage] OK"
  exit 0
} finally {
  Pop-Location
}
