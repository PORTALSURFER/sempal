<#
.SYNOPSIS
Runs the GUI automation and contract validation lane.

.DESCRIPTION
Executes the GUI action-catalog tests, GUI fixture/automation tests, and the
persistence-boundary regression that
proves GUI-oriented validation stays off the live `library.db`. The lane uses
the shared Cargo wrapper helper so inherited `sccache` or temp-directory
issues degrade cleanly to direct `rustc` instead of failing before the test
commands run.
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "../use_cargo_cache.ps1")

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

function Invoke-GuiStep {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [scriptblock]$Command
  )

  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "[gui-contract] Step failed ($Label) with exit code $LASTEXITCODE."
  }
}

Push-Location $rootDir
try {
  Enable-WavecrateCargoCache

  Write-Host "[gui-contract] cargo test app_core::actions::tests"
  Invoke-GuiStep -Label "cargo test app_core::actions::tests" -Command {
    Invoke-WavecrateCargo test app_core::actions::tests
  }

  Write-Host "[gui-contract] cargo test gui_test::"
  Invoke-GuiStep -Label "cargo test gui_test::" -Command {
    Invoke-WavecrateCargo test gui_test::
  }

  Write-Host "[gui-contract] cargo test gui_test::fixtures::tests -- --ignored"
  Invoke-GuiStep -Label "cargo test gui_test::fixtures::tests -- --ignored" -Command {
    $guiFixtureArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "gui_test::fixtures::tests",
      "--",
      "--ignored"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @guiFixtureArgs
  }

  Write-Host "[gui-contract] cargo test gui_test::runner::tests -- --ignored"
  Invoke-GuiStep -Label "cargo test gui_test::runner::tests -- --ignored" -Command {
    $guiRunnerArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "gui_test::runner::tests",
      "--",
      "--ignored"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @guiRunnerArgs
  }

  Write-Host "[gui-contract] cargo test bridge_runtime fixture checks -- --ignored"
  Invoke-GuiStep -Label "cargo test bridge_runtime fixture checks -- --ignored" -Command {
    $bridgeRuntimeArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "app_core::ui_bridge::tests::bridge_runtime",
      "--",
      "--ignored"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @bridgeRuntimeArgs
  }

  Write-Host "[gui-contract] cargo test contract_smoke_pack_runs_cleanly -- --ignored --exact"
  Invoke-GuiStep -Label "cargo test contract_smoke_pack_runs_cleanly -- --ignored --exact" -Command {
    $contractSmokeArgs = @(
      "test",
      "-p",
      "wavecrate",
      "--lib",
      "gui_test::packs::tests::contract_smoke_pack_runs_cleanly",
      "--",
      "--ignored",
      "--exact"
    )
    & cargo @(Get-WavecrateCargoConfigOverrideArgs) @contractSmokeArgs
  }

  Write-Host "[gui-contract] cargo test app_core::controller::tests::persistence_boundary::"
  Invoke-GuiStep -Label "cargo test app_core::controller::tests::persistence_boundary::" -Command {
    Invoke-WavecrateCargo test app_core::controller::tests::persistence_boundary::
  }

} finally {
  Pop-Location
}
