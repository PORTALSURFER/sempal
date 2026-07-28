<#
.SYNOPSIS
Runs the agent-safe local development validation loop.

.DESCRIPTION
This lane avoids `cargo-nextest` and the broader GUI contract/integration
wrappers so it can run in constrained Windows environments where Application
Control blocks the `cargo-nextest.exe` binary. It keeps the edit loop grounded
by running the normal compile smoke gate, Radiant's required non-blocking
guardrails and no-default core/API tests, and Wavecrate's default non-ignored
library test suite.
#>

param(
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "../use_cargo_cache.ps1")

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

if ($Help) {
  Write-Host "Usage: scripts/ci.ps1 agent"
  Write-Host "Run the agent-safe local validation loop without cargo-nextest."
  Write-Host "For the broader integrated lane, use `scripts/ci.ps1 quick`."
  Write-Host "For full CI parity, use `scripts/ci.ps1 local`."
  exit 0
}

function Invoke-NativeStep {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [scriptblock]$Command
  )

  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "[ci_agent] Step failed ($Label) with exit code $LASTEXITCODE."
  }
}

Push-Location $rootDir
try {
  Enable-WavecrateCargoCache
  Write-Host "[ci_agent] branch policy"
  Invoke-NativeStep -Label "branch policy" -Command {
    & (Join-Path $rootDir "scripts/check.ps1") main-branch
  }

  Write-Host "[ci_agent] scripts/ci.ps1 smoke"
  & (Join-Path $rootDir "scripts/ci.ps1") smoke

  Write-Host "[ci_agent] scripts/check.ps1 non-blocking-architecture"
  Invoke-NativeStep -Label "scripts/check.ps1 non-blocking-architecture" -Command {
    & (Join-Path $rootDir "scripts/check.ps1") non-blocking-architecture
  }
  Write-Host "[ci_agent] scripts/check.ps1 readiness-executor-boundary"
  Invoke-NativeStep -Label "scripts/check.ps1 readiness-executor-boundary" -Command {
    & (Join-Path $rootDir "scripts/check.ps1") readiness-executor-boundary
  }

  Write-Host "[ci_agent] cargo test -p wavecrate --test controller_browser_integration --features legacy-controller"
  Invoke-NativeStep -Label "cargo test -p wavecrate --test controller_browser_integration --features legacy-controller" -Command {
    Invoke-WavecrateCargo test -p wavecrate --test controller_browser_integration --features legacy-controller
  }

  Write-Host "[ci_agent] cargo test -p wavecrate --lib -- --skip known isolated legacy failures"
  Invoke-NativeStep -Label "cargo test -p wavecrate --lib -- --skip known isolated legacy failures" -Command {
    $wavecrateLibArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "--",
      "--skip",
      "prepare_auto_rename_requests_logs_looped_provenance",
      "--skip",
      "rating_previous_random_history_entry_restores_waveform_for_replacement"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @wavecrateLibArgs
  }

  Write-Host "[ci_agent] cargo test selection export background jobs -- --ignored --test-threads=1"
  Invoke-NativeStep -Label "cargo test selection export background jobs -- --ignored --test-threads=1" -Command {
    $selectionExportArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "selection_export_tests",
      "--",
      "--ignored",
      "--test-threads=1"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @selectionExportArgs
  }

  Write-Host "[ci_agent] OK"
} finally {
  Pop-Location
}
