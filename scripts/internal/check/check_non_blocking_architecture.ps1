<#
.SYNOPSIS
Runs the required non-blocking app architecture guardrails.

.DESCRIPTION
This check pins the Radiant and Wavecrate tests that enforce the locked-down
app-facing runtime contract. It is intentionally deterministic: static
guardrails and a controlled strict-diagnostics harness are required, while
flaky timing benchmarks remain outside this lane.
#>

param(
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "../use_cargo_cache.ps1")

if ($Help) {
  Write-Host "Usage: scripts/internal/check/check_non_blocking_architecture.ps1"
  Write-Host ""
  Write-Host "Run required Radiant and Wavecrate non-blocking architecture guardrails."
  exit 0
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
$radiantDir = (& (Join-Path $rootDir 'scripts/radiant.ps1') locate | Where-Object { $_ -like 'RADIANT_DIR=*' } | ForEach-Object { $_.Substring(13) })
if (-not $radiantDir) { throw "Radiant sibling is missing or invalid" }

function Invoke-NativeStep {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [scriptblock]$Command
  )

  Write-Host ("[non_blocking_architecture] {0}" -f $Label)
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "[non_blocking_architecture] Step failed ($Label) with exit code $LASTEXITCODE."
  }
}

Push-Location $rootDir
try {
  Enable-WavecrateCargoCache

  Invoke-NativeStep -Label "Radiant synthetic blocking-token fixture" -Command {
    Invoke-WavecrateCargo test --manifest-path (Join-Path $radiantDir 'Cargo.toml') --lib guardrail_reports_file_line_and_guidance_for_blocking_tokens
  }

  Invoke-NativeStep -Label "Radiant app/runtime/example guardrails" -Command {
    Invoke-WavecrateCargo test --manifest-path (Join-Path $radiantDir 'Cargo.toml') --test generic_surface_guardrails source_quality::runtime::commands_and_app
  }

  Invoke-NativeStep -Label "Wavecrate app-facing blocking guardrail" -Command {
    Invoke-WavecrateCargo test --package wavecrate --no-default-features --test gui_boundary native_app_ui_update_paths_do_not_call_blocking_business_apis
  }

  Invoke-NativeStep -Label "Wavecrate strict slow-handler diagnostics harness" -Command {
    Invoke-WavecrateCargo test --package wavecrate --no-default-features --lib rapid_navigation_harness_keeps_ui_responsive_while_business_work_is_slow
  }

  Invoke-NativeStep -Label "Wavecrate readiness persistence boundary" -Command {
    $supervisorFiles = @(
      (Join-Path $rootDir "src/native_app/source_processing/supervisor.rs")
    ) + @(
      Get-ChildItem (Join-Path $rootDir "src/native_app/source_processing/supervisor") -File -Filter "*.rs" |
        ForEach-Object { $_.FullName }
    )
    $productionSource = (($supervisorFiles | ForEach-Object {
      Get-Content $_ -Raw
    }) -join "`n") + "`n" + (
      Get-Content (Join-Path $rootDir "src/native_app/sample_library/similarity_artifacts/worker.rs") -Raw
    )
    if ($productionSource | Select-String -Pattern "source_readiness_(sources|targets|artifacts)|(^|[^A-Za-z0-9_])analysis_jobs([^A-Za-z0-9_]|$)|readiness_managed") {
      throw "Native source processing must use ReadinessStore for readiness persistence."
    }
  }

  Invoke-NativeStep -Label "Wavecrate source-processing service boundary" -Command {
    $serviceDir = Join-Path $rootDir "src/native_app/source_processing/supervisor"
    $serviceFiles = Get-ChildItem $serviceDir -File -Filter "*.rs"
    $wildcardImports = $serviceFiles | Select-String -Pattern "^use super::\*;"
    if ($wildcardImports) {
      throw "Source-processing production modules must declare explicit contracts."
    }
    $ownedServices = $serviceFiles | Where-Object {
      $_.Name -match "^(discovery|execution|retirement)" -or
      $_.Name -in @("progress.rs", "telemetry.rs")
    }
    $facadeCrossings = $ownedServices | Select-String -Pattern "SourceProcessingSupervisor|run_coordinator|CoordinatorExecutionState|execute_candidates"
    if ($facadeCrossings) {
      throw "Source-processing services must not reach into the facade or coordinator."
    }
  }

  Write-Host "[non_blocking_architecture] OK"
  exit 0
} finally {
  Pop-Location
}
