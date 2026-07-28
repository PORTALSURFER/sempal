<#
.SYNOPSIS
Runs the fastest local compile/smoke check.

.DESCRIPTION
Executes the smallest useful development gate by type-checking library, test,
and binary targets without running the test suite. Use this during normal edit
loops, then escalate to `scripts/ci.ps1 quick` or `scripts/ci.ps1 local` when
the change risk warrants broader validation.
#>

param(
  [switch]$AppOnly,
  [switch]$Workspace,
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "../use_cargo_cache.ps1")

if ($Help) {
  Write-Host "Usage: scripts/ci.ps1 smoke [-AppOnly] [-Workspace]"
  Write-Host "Run the compile/smoke gate."
  Write-Host "Use -AppOnly for the lightest app-only check."
  Write-Host "Use -Workspace for the broad workspace compile gate."
  Write-Host "For fast test coverage, use `scripts/ci.ps1 quick`."
  exit 0
}

if ($AppOnly -and $Workspace) {
  throw "AppOnly and Workspace are mutually exclusive."
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

function Invoke-NativeStep {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [scriptblock]$Command
  )

  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "[devcheck] Step failed ($Label) with exit code $LASTEXITCODE."
  }
}

Push-Location $rootDir
try {
  Enable-WavecrateCargoCache
  Write-Host "[devcheck] branch policy"
  Invoke-NativeStep -Label "branch policy" -Command {
    & (Join-Path $rootDir "scripts/check.ps1") main-branch
  }

  Write-Host "[devcheck] cargo metadata --locked"
  Invoke-NativeStep -Label "cargo metadata --locked" -Command {
    Invoke-WavecrateCargo metadata --locked --format-version 1
  }

  if ($AppOnly) {
    Write-Host "[devcheck] cargo check --package wavecrate --lib --bin wavecrate"
    Invoke-NativeStep -Label "cargo check --package wavecrate --lib --bin wavecrate" -Command {
      Invoke-WavecrateCargo check --package wavecrate --lib --bin wavecrate
    }
  } elseif ($Workspace) {
    Write-Host "[devcheck] cargo check --workspace --tests --bins"
    Invoke-NativeStep -Label "cargo check --workspace --tests --bins" -Command {
      Invoke-WavecrateCargo check --workspace --tests --bins
    }
  } else {
    Write-Host "[devcheck] cargo check --package wavecrate --tests --bins"
    Invoke-NativeStep -Label "cargo check --package wavecrate --tests --bins" -Command {
      Invoke-WavecrateCargo check --package wavecrate --tests --bins
    }
  }

  Write-Host "[devcheck] OK"
} finally {
  Pop-Location
}
